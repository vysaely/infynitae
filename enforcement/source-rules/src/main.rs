//! Structural source rules for `tools/verify-architecture` (OB-19).
//!
//! Each crate is loaded from its root file by following `mod` declarations, as rustc does, and
//! parsed with `syn`. Items gated on a `probe_*` feature are removed first: they are compiled only
//! by the negative probes. The rules then run on the syntax tree, not on text, so formatting,
//! comments or line breaks cannot hide a violation.
//!
//! Usage: `infynitae-source-rules [--root <workspace root>]`. Exit status 0 when every rule holds.

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::visit_mut::VisitMut;

/// Authority values minted only by `Core::new` (C1.5, INV-OUTPUT-2, C12.R2.3).
const PORTS: [&str; 3] = ["InputPort", "ShellPort", "OutputLifecycleToken"];
/// The proof type (INV-CAMERA-1).
const PROOF: &str = "Authorized";
/// Entry points inside which a proof may be minted (INV-CAMERA-1).
const MINT_ENTRIES: [&str; 4] = [
    "physical_input",
    "shell_command",
    "output_connected",
    "output_disconnected",
];
/// Names the Smithay adapter must never use (OB-19, C12.R2.2; `output_reconfigured` from
/// INV-OUTPUT-2, which admits reconfiguration only through the token the adapter never holds).
const WAYLAND_FORBIDDEN: [&str; 8] = [
    "InputPort",
    "ShellPort",
    "OutputLifecycleToken",
    "physical_input",
    "shell_command",
    "output_connected",
    "output_reconfigured",
    "output_disconnected",
];
/// Identifiers that would give `output_reconfigured` camera authority (E1R, verify.sh step 2h).
const RECONFIGURE_FORBIDDEN: [&str; 4] = ["mint", "Authorized", "navigation", "set_center"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnsafeLevel {
    Forbid,
    Deny,
}

/// Every workspace crate, its root file and its required `unsafe_code` level (C9, INV-ARCH-2).
const CRATES: [(&str, &str, UnsafeLevel); 8] = [
    (
        "infynitae-core",
        "crates/infynitae-core/src/lib.rs",
        UnsafeLevel::Forbid,
    ),
    (
        "infynitae-ipc",
        "crates/infynitae-ipc/src/lib.rs",
        UnsafeLevel::Forbid,
    ),
    (
        "infynitae-wayland",
        "crates/infynitae-wayland/src/lib.rs",
        UnsafeLevel::Deny,
    ),
    (
        "infynitae-backend-headless",
        "crates/infynitae-backend-headless/src/lib.rs",
        UnsafeLevel::Deny,
    ),
    (
        "infynitae-backend-winit",
        "crates/infynitae-backend-winit/src/lib.rs",
        UnsafeLevel::Deny,
    ),
    (
        "infynitae",
        "crates/infynitae/src/main.rs",
        UnsafeLevel::Deny,
    ),
    (
        "infynitae-architecture-probes",
        "enforcement/probes/src/lib.rs",
        UnsafeLevel::Deny,
    ),
    (
        "infynitae-source-rules",
        "enforcement/source-rules/src/main.rs",
        UnsafeLevel::Deny,
    ),
];

const UNSAFE_INVENTORY: &str = "docs/foundation/UNSAFE_INVENTORY.md";

struct SourceFile {
    path: PathBuf,
    module: Vec<String>,
    ast: syn::File,
}

#[derive(Default)]
struct Report {
    findings: BTreeMap<&'static str, Vec<String>>,
}

impl Report {
    fn rule(&mut self, rule: &'static str) {
        self.findings.entry(rule).or_default();
    }
    fn fail(&mut self, rule: &'static str, msg: String) {
        self.findings.entry(rule).or_default().push(msg);
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut root = PathBuf::from(".");
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--root", Some(r)) => root = PathBuf::from(r),
            _ => {
                eprintln!("usage: infynitae-source-rules [--root <workspace root>]");
                return ExitCode::from(2);
            }
        }
    }
    let mut report = Report::default();
    let mut crates = BTreeMap::new();
    for (name, root_file, _) in CRATES {
        match load_crate(&root.join(root_file)) {
            Ok(files) => {
                crates.insert(name, files);
            }
            Err(e) => report.fail("load", format!("{name}: {e}")),
        }
    }
    report.rule("load");
    if let Some(core) = crates.get("infynitae-core") {
        check_core(core, &mut report);
    }
    if let Some(wayland) = crates.get("infynitae-wayland") {
        check_wayland(wayland, &mut report);
    }
    check_unsafe(&root, &crates, &mut report);

    let mut failed = false;
    for (rule, msgs) in &report.findings {
        if msgs.is_empty() {
            println!("ok   {rule}");
        } else {
            failed = true;
            for m in msgs {
                println!("FAIL {rule}: {m}");
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------- loading

fn parse(path: &Path) -> Result<syn::File, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut ast = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    StripProbes.visit_file_mut(&mut ast);
    Ok(ast)
}

/// Loads a crate by following non-inline `mod` declarations from its root file.
fn load_crate(root_file: &Path) -> Result<Vec<SourceFile>, String> {
    let mut out = Vec::new();
    let ast = parse(root_file)?;
    let dir = root_file.parent().unwrap_or(Path::new(".")).to_path_buf();
    collect_mods(&ast.items, &[], &dir, &mut out)?;
    out.insert(
        0,
        SourceFile {
            path: root_file.to_path_buf(),
            module: vec![],
            ast,
        },
    );
    Ok(out)
}

fn collect_mods(
    items: &[syn::Item],
    module: &[String],
    dir: &Path,
    out: &mut Vec<SourceFile>,
) -> Result<(), String> {
    for item in items {
        let syn::Item::Mod(m) = item else { continue };
        if m.attrs.iter().any(|a| a.path().is_ident("path")) {
            return Err(format!(
                "#[path] module `{}` is not supported (fail closed)",
                m.ident
            ));
        }
        let name = m.ident.to_string();
        let mut sub = module.to_vec();
        sub.push(name.clone());
        match &m.content {
            Some((_, inner)) => collect_mods(inner, &sub, &dir.join(&name), out)?,
            None => {
                let flat = dir.join(format!("{name}.rs"));
                let nested = dir.join(&name).join("mod.rs");
                let path = if flat.exists() { flat } else { nested };
                let ast = parse(&path)?;
                collect_mods(&ast.items, &sub, &dir.join(&name), out)?;
                out.push(SourceFile {
                    path,
                    module: sub,
                    ast,
                });
            }
        }
    }
    Ok(())
}

/// True for `#[cfg(feature = "probe_…")]` or `#[cfg(any(feature = "probe_…", …))]` only.
fn probe_gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        let Ok(list) = a.meta.require_list() else {
            return false;
        };
        let (mut idents, mut lits) = (Vec::new(), Vec::new());
        flatten(list.tokens.clone(), &mut idents, &mut lits);
        !lits.is_empty()
            && idents.iter().all(|i| i == "any" || i == "feature")
            && lits.iter().all(|l| l.starts_with("\"probe_"))
    })
}

fn flatten(ts: TokenStream, idents: &mut Vec<String>, lits: &mut Vec<String>) {
    for t in ts {
        match t {
            TokenTree::Group(g) => flatten(g.stream(), idents, lits),
            TokenTree::Ident(i) => idents.push(i.to_string()),
            TokenTree::Literal(l) => lits.push(l.to_string()),
            TokenTree::Punct(_) => {}
        }
    }
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    use syn::Item::*;
    match item {
        Const(i) => &i.attrs,
        Enum(i) => &i.attrs,
        ExternCrate(i) => &i.attrs,
        Fn(i) => &i.attrs,
        ForeignMod(i) => &i.attrs,
        Impl(i) => &i.attrs,
        Macro(i) => &i.attrs,
        Mod(i) => &i.attrs,
        Static(i) => &i.attrs,
        Struct(i) => &i.attrs,
        Trait(i) => &i.attrs,
        TraitAlias(i) => &i.attrs,
        Type(i) => &i.attrs,
        Union(i) => &i.attrs,
        Use(i) => &i.attrs,
        _ => &[],
    }
}

fn impl_item_attrs(item: &syn::ImplItem) -> &[syn::Attribute] {
    match item {
        syn::ImplItem::Const(i) => &i.attrs,
        syn::ImplItem::Fn(i) => &i.attrs,
        syn::ImplItem::Type(i) => &i.attrs,
        syn::ImplItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

fn stmt_gated(s: &syn::Stmt) -> bool {
    match s {
        syn::Stmt::Local(l) => probe_gated(&l.attrs),
        syn::Stmt::Item(i) => probe_gated(item_attrs(i)),
        syn::Stmt::Macro(m) => probe_gated(&m.attrs),
        syn::Stmt::Expr(syn::Expr::Block(b), _) => probe_gated(&b.attrs),
        syn::Stmt::Expr(..) => false,
    }
}

fn retain<T, P: Default>(p: &mut Punctuated<T, P>, keep: impl Fn(&T) -> bool) {
    *p = std::mem::take(p).into_iter().filter(|t| keep(t)).collect();
}

/// Removes every syntax node gated on a probe feature.
struct StripProbes;

impl VisitMut for StripProbes {
    fn visit_file_mut(&mut self, f: &mut syn::File) {
        f.items.retain(|i| !probe_gated(item_attrs(i)));
        syn::visit_mut::visit_file_mut(self, f);
    }
    fn visit_item_mod_mut(&mut self, m: &mut syn::ItemMod) {
        if let Some((_, items)) = &mut m.content {
            items.retain(|i| !probe_gated(item_attrs(i)));
        }
        syn::visit_mut::visit_item_mod_mut(self, m);
    }
    fn visit_item_impl_mut(&mut self, i: &mut syn::ItemImpl) {
        i.items.retain(|it| !probe_gated(impl_item_attrs(it)));
        syn::visit_mut::visit_item_impl_mut(self, i);
    }
    fn visit_block_mut(&mut self, b: &mut syn::Block) {
        b.stmts.retain(|s| !stmt_gated(s));
        syn::visit_mut::visit_block_mut(self, b);
    }
    fn visit_fields_named_mut(&mut self, f: &mut syn::FieldsNamed) {
        retain(&mut f.named, |x| !probe_gated(&x.attrs));
        syn::visit_mut::visit_fields_named_mut(self, f);
    }
    fn visit_fields_unnamed_mut(&mut self, f: &mut syn::FieldsUnnamed) {
        retain(&mut f.unnamed, |x| !probe_gated(&x.attrs));
        syn::visit_mut::visit_fields_unnamed_mut(self, f);
    }
    fn visit_item_enum_mut(&mut self, e: &mut syn::ItemEnum) {
        retain(&mut e.variants, |v| !probe_gated(&v.attrs));
        syn::visit_mut::visit_item_enum_mut(self, e);
    }
    fn visit_expr_struct_mut(&mut self, e: &mut syn::ExprStruct) {
        retain(&mut e.fields, |f| !probe_gated(&f.attrs));
        syn::visit_mut::visit_expr_struct_mut(self, e);
    }
    fn visit_expr_match_mut(&mut self, m: &mut syn::ExprMatch) {
        m.arms.retain(|a| !probe_gated(&a.attrs));
        syn::visit_mut::visit_expr_match_mut(self, m);
    }
}

// ---------------------------------------------------------------- helpers

fn at(path: &Path, span: proc_macro2::Span) -> String {
    format!("{}:{}", path.display(), span.start().line)
}

/// Every identifier in a token stream, groups included.
fn idents(ts: TokenStream, out: &mut Vec<proc_macro2::Ident>) {
    for t in ts {
        match t {
            TokenTree::Group(g) => idents(g.stream(), out),
            TokenTree::Ident(i) => out.push(i),
            _ => {}
        }
    }
}

fn last_ident(path: &syn::Path) -> Option<String> {
    path.segments.last().map(|s| s.ident.to_string())
}

/// Type names that appear by value (not behind a reference) in a type.
fn by_value_names(ty: &syn::Type, out: &mut Vec<String>) {
    match ty {
        syn::Type::Reference(_) | syn::Type::Ptr(_) => {}
        syn::Type::Tuple(t) => t.elems.iter().for_each(|e| by_value_names(e, out)),
        syn::Type::Paren(p) => by_value_names(&p.elem, out),
        syn::Type::Group(g) => by_value_names(&g.elem, out),
        syn::Type::Array(a) => by_value_names(&a.elem, out),
        syn::Type::Slice(s) => by_value_names(&s.elem, out),
        syn::Type::Path(p) => {
            for seg in &p.path.segments {
                out.push(seg.ident.to_string());
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    for a in &args.args {
                        if let syn::GenericArgument::Type(t) = a {
                            by_value_names(t, out);
                        }
                    }
                }
            }
        }
        other => {
            let mut ids = Vec::new();
            idents(other.to_token_stream(), &mut ids);
            out.extend(ids.iter().map(|i| i.to_string()));
        }
    }
}

// ---------------------------------------------------------------- core rules

#[derive(Clone, Default)]
struct Ctx {
    module: Vec<String>,
    impl_ty: Option<String>,
    func: Option<String>,
}

#[derive(Default)]
struct CoreFacts {
    /// (type, context, location) of every struct literal of a port, the token or the proof
    constructions: Vec<(String, Ctx, String)>,
    /// (context, location) of every use of `mint`
    mint_uses: Vec<(Ctx, String)>,
    /// (context, visibility, location) of every `fn mint`
    mint_defs: Vec<(Ctx, String, String)>,
    /// (function, returned authority type, location)
    returns: Vec<(Ctx, String, String)>,
    /// `Clone`/`Copy` on an authority type: (type, location)
    duplicable: Vec<(String, String)>,
    /// identifiers in each `Core::output_reconfigured` body: (location, identifiers)
    reconfigured: Vec<(String, Vec<proc_macro2::Ident>)>,
}

struct CoreVisitor<'a> {
    file: &'a Path,
    ctx: Ctx,
    facts: &'a mut CoreFacts,
}

fn authority_type(name: &str) -> bool {
    PORTS.contains(&name) || name == PROOF
}

impl CoreVisitor<'_> {
    fn check_fn(&mut self, sig: &syn::Signature) {
        if let syn::ReturnType::Type(_, ty) = &sig.output {
            let mut names = Vec::new();
            by_value_names(ty, &mut names);
            for n in names.into_iter().filter(|n| authority_type(n)) {
                self.facts
                    .returns
                    .push((self.ctx.clone(), n, at(self.file, sig.ident.span())));
            }
        }
    }
    fn derives(&mut self, attrs: &[syn::Attribute], ty: &syn::Ident) {
        if !authority_type(&ty.to_string()) {
            return;
        }
        for a in attrs.iter().filter(|a| a.path().is_ident("derive")) {
            let mut ids = Vec::new();
            idents(a.meta.to_token_stream(), &mut ids);
            if ids.iter().any(|i| i == "Clone" || i == "Copy") {
                self.facts
                    .duplicable
                    .push((ty.to_string(), at(self.file, ty.span())));
            }
        }
    }
}

impl<'ast> Visit<'ast> for CoreVisitor<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if m.content.is_some() {
            let saved = self.ctx.clone();
            self.ctx.module.push(m.ident.to_string());
            syn::visit::visit_item_mod(self, m);
            self.ctx = saved;
        }
    }
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let saved = self.ctx.clone();
        let self_ty = match &*i.self_ty {
            syn::Type::Path(p) => last_ident(&p.path),
            _ => None,
        };
        if let (Some((_, tr, _)), Some(t)) = (&i.trait_, &self_ty) {
            let tr = last_ident(tr).unwrap_or_default();
            if (tr == "Clone" || tr == "Copy") && authority_type(t) {
                self.facts
                    .duplicable
                    .push((t.clone(), at(self.file, i.impl_token.span)));
            }
        }
        self.ctx.impl_ty = self_ty;
        syn::visit::visit_item_impl(self, i);
        self.ctx = saved;
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let saved = self.ctx.clone();
        self.ctx.func = Some(f.sig.ident.to_string());
        self.check_fn(&f.sig);
        if f.sig.ident == "mint" {
            let vis = f.vis.to_token_stream().to_string().replace(' ', "");
            self.facts
                .mint_defs
                .push((self.ctx.clone(), vis, at(self.file, f.sig.ident.span())));
        }
        if f.sig.ident == "output_reconfigured" && self.ctx.impl_ty.as_deref() == Some("Core") {
            let mut ids = Vec::new();
            idents(f.block.to_token_stream(), &mut ids);
            self.facts
                .reconfigured
                .push((at(self.file, f.sig.ident.span()), ids));
        }
        syn::visit::visit_impl_item_fn(self, f);
        self.ctx = saved;
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let saved = self.ctx.clone();
        self.ctx.impl_ty = None;
        self.ctx.func = Some(f.sig.ident.to_string());
        self.check_fn(&f.sig);
        syn::visit::visit_item_fn(self, f);
        self.ctx = saved;
    }
    fn visit_item_struct(&mut self, s: &'ast syn::ItemStruct) {
        self.derives(&s.attrs, &s.ident);
        syn::visit::visit_item_struct(self, s);
    }
    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        self.derives(&e.attrs, &e.ident);
        syn::visit::visit_item_enum(self, e);
    }
    fn visit_expr_struct(&mut self, e: &'ast syn::ExprStruct) {
        if let Some(t) = last_ident(&e.path).filter(|t| authority_type(t)) {
            let loc = at(
                self.file,
                e.path
                    .segments
                    .last()
                    .map_or(e.brace_token.span.join(), |s| s.ident.span()),
            );
            self.facts.constructions.push((t, self.ctx.clone(), loc));
        }
        syn::visit::visit_expr_struct(self, e);
    }
    fn visit_path(&mut self, p: &'ast syn::Path) {
        if let Some(seg) = p.segments.last().filter(|s| s.ident == "mint") {
            self.facts
                .mint_uses
                .push((self.ctx.clone(), at(self.file, seg.ident.span())));
        }
        syn::visit::visit_path(self, p);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        // Macro bodies are not parsed: a literal of an authority type (`Name {`) or any `mint`
        // token inside a macro counts as a construction or a mint use.
        let tokens: Vec<TokenTree> = m.tokens.clone().into_iter().collect();
        scan_macro(&tokens, self);
        syn::visit::visit_macro(self, m);
    }
}

fn scan_macro(tokens: &[TokenTree], v: &mut CoreVisitor<'_>) {
    for (k, t) in tokens.iter().enumerate() {
        match t {
            TokenTree::Group(g) => {
                let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                scan_macro(&inner, v);
            }
            TokenTree::Ident(i) if i == "mint" => v
                .facts
                .mint_uses
                .push((v.ctx.clone(), at(v.file, i.span()))),
            TokenTree::Ident(i) if authority_type(&i.to_string()) => {
                if matches!(tokens.get(k + 1), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace)
                {
                    v.facts.constructions.push((
                        i.to_string(),
                        v.ctx.clone(),
                        at(v.file, i.span()),
                    ));
                }
            }
            _ => {}
        }
    }
}

fn check_core(files: &[SourceFile], r: &mut Report) {
    let mut facts = CoreFacts::default();
    for f in files {
        let mut v = CoreVisitor {
            file: &f.path,
            ctx: Ctx {
                module: f.module.clone(),
                ..Ctx::default()
            },
            facts: &mut facts,
        };
        v.visit_file(&f.ast);
    }
    let core_new =
        |c: &Ctx| c.impl_ty.as_deref() == Some("Core") && c.func.as_deref() == Some("new");
    let in_mint = |c: &Ctx| {
        c.impl_ty.as_deref() == Some(PROOF)
            && c.func.as_deref() == Some("mint")
            && c.module == ["entry", "authority"]
    };

    // 2g (C12.R2.3), generalised to both ports: one construction site each, inside Core::new.
    const R1: &str = "authority-construction";
    r.rule(R1);
    for ty in PORTS.iter().copied().chain([PROOF]) {
        let sites: Vec<_> = facts
            .constructions
            .iter()
            .filter(|(t, _, _)| t == ty)
            .collect();
        let allowed = |c: &Ctx| if ty == PROOF { in_mint(c) } else { core_new(c) };
        for (_, c, loc) in sites.iter().filter(|(_, c, _)| !allowed(c)) {
            let where_ = c.func.clone().unwrap_or_else(|| "<item>".into());
            r.fail(
                R1,
                format!(
                    "{loc}: `{ty}` constructed in `{where_}`; only {} may construct it",
                    if ty == PROOF {
                        "entry::authority::Authorized::mint"
                    } else {
                        "Core::new"
                    }
                ),
            );
        }
        if sites.len() != 1 {
            r.fail(
                R1,
                format!(
                    "`{ty}` has {} construction sites; exactly 1 required",
                    sites.len()
                ),
            );
        }
    }

    // No function other than Core::new returns a port or the token; only mint returns a proof.
    const R2: &str = "authority-return";
    r.rule(R2);
    for (c, ty, loc) in &facts.returns {
        let ok = if ty == PROOF { in_mint(c) } else { core_new(c) };
        if !ok {
            r.fail(
                R2,
                format!(
                    "{loc}: `{}` returns `{ty}` by value",
                    c.func.clone().unwrap_or_default()
                ),
            );
        }
    }

    // Ports, token and proof are never Clone or Copy (C1.5, INV-CAMERA-1, INV-OUTPUT-2).
    const R3: &str = "authority-not-duplicable";
    r.rule(R3);
    for (ty, loc) in &facts.duplicable {
        r.fail(
            R3,
            format!("{loc}: `{ty}` implements or derives Clone/Copy"),
        );
    }

    // `Authorized::mint` is defined once, private to `entry`, and used only in the four
    // proof-minting entry points (OB-19 source rule; INV-CAMERA-1).
    const R4: &str = "mint-location";
    r.rule(R4);
    match facts.mint_defs.as_slice() {
        [(c, vis, loc)] => {
            if !(c.impl_ty.as_deref() == Some(PROOF) && c.module == ["entry", "authority"]) {
                r.fail(
                    R4,
                    format!("{loc}: `mint` is not defined on `Authorized` in `entry::authority`"),
                );
            }
            if vis != "pub(incrate::entry)" {
                r.fail(
                    R4,
                    format!(
                        "{loc}: `mint` has visibility `{vis}`; `pub(in crate::entry)` required"
                    ),
                );
            }
        }
        defs => r.fail(
            R4,
            format!(
                "{} definitions of `fn mint`; exactly 1 required",
                defs.len()
            ),
        ),
    }
    for (c, loc) in &facts.mint_uses {
        let ok = c.module.first().map(String::as_str) == Some("entry")
            && c.impl_ty.as_deref() == Some("Core")
            && c.func.as_deref().is_some_and(|f| MINT_ENTRIES.contains(&f));
        if !ok {
            r.fail(
                R4,
                format!(
                    "{loc}: `mint` used in `{}`; only {MINT_ENTRIES:?} in `entry` may mint",
                    c.func.clone().unwrap_or_default()
                ),
            );
        }
    }

    // E1R 2h: output reconfiguration carries no camera authority.
    const R5: &str = "reconfigured-no-camera-authority";
    r.rule(R5);
    match facts.reconfigured.as_slice() {
        [(loc, ids)] => {
            for i in ids
                .iter()
                .filter(|i| RECONFIGURE_FORBIDDEN.iter().any(|f| *i == f))
            {
                r.fail(
                    R5,
                    format!(
                        "{loc}: `output_reconfigured` body names `{i}` (line {})",
                        i.span().start().line
                    ),
                );
            }
        }
        defs => r.fail(
            R5,
            format!(
                "{} definitions of `Core::output_reconfigured`; exactly 1 required",
                defs.len()
            ),
        ),
    }
}

// ---------------------------------------------------------------- wayland rule

fn check_wayland(files: &[SourceFile], r: &mut Report) {
    const R: &str = "wayland-names-no-authority";
    r.rule(R);
    for f in files {
        let mut ids = Vec::new();
        idents(f.ast.to_token_stream(), &mut ids);
        for i in ids
            .iter()
            .filter(|i| WAYLAND_FORBIDDEN.iter().any(|w| *i == w))
        {
            r.fail(R, format!("{}: names `{i}`", at(&f.path, i.span())));
        }
    }
}

// ---------------------------------------------------------------- unsafe policy

fn check_unsafe(root: &Path, crates: &BTreeMap<&str, Vec<SourceFile>>, r: &mut Report) {
    const R: &str = "unsafe-policy";
    r.rule(R);
    let inventory = root.join(UNSAFE_INVENTORY).exists();
    for (name, _, level) in CRATES {
        let Some(files) = crates.get(name) else {
            continue;
        };
        let root_file = &files[0];
        let want = if level == UnsafeLevel::Forbid {
            "forbid"
        } else {
            "deny"
        };
        let has = root_file.ast.attrs.iter().any(|a| {
            matches!(a.style, syn::AttrStyle::Inner(_)) && a.path().is_ident(want) && {
                let mut ids = Vec::new();
                idents(a.meta.to_token_stream(), &mut ids);
                ids.iter().any(|i| i == "unsafe_code")
            }
        });
        if !has {
            r.fail(
                R,
                format!(
                    "{}: `#![{want}(unsafe_code)]` missing at the crate root ({name})",
                    root_file.path.display()
                ),
            );
        }
        for f in files {
            let mut tokens = Vec::new();
            relax_and_unsafe(f.ast.to_token_stream(), &mut tokens);
            for (what, span) in tokens {
                let msg = format!("{}: {what}", at(&f.path, span));
                if !inventory {
                    r.fail(R, format!("{msg} without {UNSAFE_INVENTORY} (C9)"));
                } else if level == UnsafeLevel::Forbid {
                    r.fail(R, format!("{msg} in a forbid crate"));
                }
            }
        }
    }
}

/// `unsafe` keywords and `allow`/`expect`/`warn` attributes naming `unsafe_code`.
fn relax_and_unsafe(ts: TokenStream, out: &mut Vec<(String, proc_macro2::Span)>) {
    let tokens: Vec<TokenTree> = ts.into_iter().collect();
    for (k, t) in tokens.iter().enumerate() {
        match t {
            TokenTree::Ident(i) if i == "unsafe" => out.push(("`unsafe`".into(), i.span())),
            TokenTree::Ident(i) if i == "allow" || i == "expect" || i == "warn" => {
                if let Some(TokenTree::Group(g)) = tokens.get(k + 1) {
                    let mut ids = Vec::new();
                    idents(g.stream(), &mut ids);
                    if ids.iter().any(|x| x == "unsafe_code") {
                        out.push((format!("`{i}(unsafe_code)`"), i.span()));
                    }
                }
            }
            TokenTree::Group(g) => relax_and_unsafe(g.stream(), out),
            _ => {}
        }
    }
}
