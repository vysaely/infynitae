//! Structural source rules for `tools/verify-architecture` (OB-19).
//!
//! Each crate is loaded from its root file by following `mod` declarations, as rustc does, and
//! parsed with `syn`. Items gated on a `probe_*` feature are removed first: they are compiled only
//! by the negative probes. The rules then run on the syntax tree, not on text, so formatting,
//! comments or line breaks cannot hide a violation.
//!
//! Usage: `infynitae-source-rules [--root <workspace root>]`. Exit status 0 when every rule holds.

#![deny(unsafe_code)]

use std::collections::{BTreeMap, HashSet};
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

/// Smithay APIs the architecture rejects in product code (OB-26): `Space` (W-SM-1, ADR-01),
/// `Window::surface_under` (W-SM-11/12, INV-HIT-1), `render_elements_from_surface_tree` with its
/// i32 root (W-SM-5, F-RENDER-1), `Window::output_enter/leave` (INV-RENDER-3).
const REJECTED_SMITHAY: [&str; 5] = [
    "Space",
    "surface_under",
    "render_elements_from_surface_tree",
    "output_enter",
    "output_leave",
];
/// Smithay geometry types: constructed and named only in the adapter's `convert` module
/// (INV-COORD-4, C3.6).
const SMITHAY_GEOMETRY: [&str; 6] = ["Point", "Rectangle", "Size", "Scale", "Logical", "Physical"];
/// Product crates (the first six of `CRATES`).
const PRODUCT_CRATES: usize = 6;
/// Stage writers (INV-STAGE-1): callable only from `core::protocol` (lifecycle and placement policy).
const STAGE_WRITERS: [&str; 6] = [
    "create",
    "place",
    "set_state",
    "set_tree",
    "raise",
    "destroy",
];

/// Smithay pointer delivery (INC-3, C4.2): called only by the adapter's `pointer` module, which
/// applies Core's pointer effects. Anything else calling them would route input around Core
/// (INV-INPUT-1/3/4).
const POINTER_DELIVERY: [&str; 8] = [
    "motion",
    "relative_motion",
    "button",
    "axis",
    "frame",
    "set_grab",
    "unset_grab",
    "get_pointer",
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
        check_stage_writers(core, &mut report);
    }
    if let Some(wayland) = crates.get("infynitae-wayland") {
        check_wayland(wayland, &mut report);
        check_wayland_geometry(wayland, &mut report);
    }
    check_rejected_apis(&crates, &mut report);
    check_effects_only_from_core(&crates, &mut report);
    check_pointer_delivery(&crates, &mut report);
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

/// Type names that appear in a type (by value, reference, pointer, tuple, generics, etc.).
fn authority_names_in_type(ty: &syn::Type, out: &mut Vec<String>, impl_ty: Option<&str>) {
    match ty {
        syn::Type::Reference(r) => authority_names_in_type(&r.elem, out, impl_ty),
        syn::Type::Ptr(p) => authority_names_in_type(&p.elem, out, impl_ty),
        syn::Type::Tuple(t) => t
            .elems
            .iter()
            .for_each(|e| authority_names_in_type(e, out, impl_ty)),
        syn::Type::Paren(p) => authority_names_in_type(&p.elem, out, impl_ty),
        syn::Type::Group(g) => authority_names_in_type(&g.elem, out, impl_ty),
        syn::Type::Array(a) => authority_names_in_type(&a.elem, out, impl_ty),
        syn::Type::Slice(s) => authority_names_in_type(&s.elem, out, impl_ty),
        syn::Type::BareFn(f) => {
            if let syn::ReturnType::Type(_, ret) = &f.output {
                authority_names_in_type(ret, out, impl_ty);
            }
        }
        syn::Type::Path(p) => {
            for seg in &p.path.segments {
                let id = seg.ident.to_string();
                if id == "Self" {
                    if let Some(t) = impl_ty {
                        out.push(t.to_string());
                    }
                } else {
                    out.push(id);
                }
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    for a in &args.args {
                        if let syn::GenericArgument::Type(t) = a {
                            authority_names_in_type(t, out, impl_ty);
                        }
                    }
                }
            }
        }
        other => {
            let mut ids = Vec::new();
            idents(other.to_token_stream(), &mut ids);
            for i in ids {
                let id = i.to_string();
                if id == "Self" {
                    if let Some(t) = impl_ty {
                        out.push(t.to_string());
                    }
                } else {
                    out.push(id);
                }
            }
        }
    }
}

/// Collect authority types and their aliases/re-exports within core.
fn collect_authority_types(files: &[SourceFile]) -> HashSet<String> {
    let mut authority: HashSet<String> = PORTS
        .iter()
        .copied()
        .chain([PROOF])
        .map(String::from)
        .collect();
    let mut changed = true;
    while changed {
        changed = false;
        for f in files {
            struct AliasVisitor<'a> {
                auth: &'a mut HashSet<String>,
                changed: &'a mut bool,
            }
            impl<'ast> Visit<'ast> for AliasVisitor<'_> {
                fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
                    let mut names = Vec::new();
                    authority_names_in_type(&i.ty, &mut names, None);
                    if names.iter().any(|n| self.auth.contains(n))
                        && self.auth.insert(i.ident.to_string())
                    {
                        *self.changed = true;
                    }
                    syn::visit::visit_item_type(self, i);
                }
                fn visit_use_tree(&mut self, u: &'ast syn::UseTree) {
                    match u {
                        syn::UseTree::Path(p) => self.visit_use_tree(&p.tree),
                        syn::UseTree::Group(g) => {
                            for item in &g.items {
                                self.visit_use_tree(item);
                            }
                        }
                        syn::UseTree::Rename(r)
                            if self.auth.contains(&r.ident.to_string())
                                && self.auth.insert(r.rename.to_string()) =>
                        {
                            *self.changed = true;
                        }
                        _ => {}
                    }
                }
            }
            let mut v = AliasVisitor {
                auth: &mut authority,
                changed: &mut changed,
            };
            v.visit_file(&f.ast);
        }
    }
    authority
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
    authority_types: &'a HashSet<String>,
}

impl CoreVisitor<'_> {
    fn authority_type(&self, name: &str) -> bool {
        self.authority_types.contains(name)
    }

    fn check_fn(&mut self, sig: &syn::Signature) {
        if let syn::ReturnType::Type(_, ty) = &sig.output {
            let mut names = Vec::new();
            authority_names_in_type(ty, &mut names, self.ctx.impl_ty.as_deref());
            names.retain(|n| self.authority_type(n));
            for n in names {
                self.facts
                    .returns
                    .push((self.ctx.clone(), n, at(self.file, sig.ident.span())));
            }
        }
    }
    fn derives(&mut self, attrs: &[syn::Attribute], ty: &syn::Ident) {
        if !self.authority_type(&ty.to_string()) {
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
            if (tr == "Clone" || tr == "Copy") && self.authority_type(t) {
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
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        let saved = self.ctx.clone();
        self.ctx.func = Some(f.sig.ident.to_string());
        self.check_fn(&f.sig);
        syn::visit::visit_trait_item_fn(self, f);
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
        if let Some(t) = last_ident(&e.path).filter(|t| self.authority_type(t)) {
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
            TokenTree::Ident(i) if v.authority_type(&i.to_string()) => {
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
    let authority_set = collect_authority_types(files);
    let mut facts = CoreFacts::default();
    for f in files {
        let mut v = CoreVisitor {
            file: &f.path,
            ctx: Ctx {
                module: f.module.clone(),
                ..Ctx::default()
            },
            facts: &mut facts,
            authority_types: &authority_set,
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
                    "{loc}: `{}` returns `{ty}` (authority escape)",
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

// ---------------------------------------------------------------- Stage single writer

struct StageWriters<'a> {
    file: &'a Path,
    module: Vec<String>,
    out: Vec<String>,
}

impl<'ast> Visit<'ast> for StageWriters<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if m.content.is_some() {
            self.module.push(m.ident.to_string());
            syn::visit::visit_item_mod(self, m);
            self.module.pop();
        }
    }
    fn visit_expr_method_call(&mut self, c: &'ast syn::ExprMethodCall) {
        let on_stage = matches!(&*c.receiver, syn::Expr::Field(f)
            if matches!(&f.member, syn::Member::Named(n) if n == "stage"));
        let writer = STAGE_WRITERS.iter().any(|w| c.method == w);
        if on_stage && writer && self.module.first().map(String::as_str) != Some("protocol") {
            self.out.push(format!(
                "{}: `stage.{}` called outside `core::protocol` (INV-STAGE-1 single writer)",
                at(self.file, c.method.span()),
                c.method
            ));
        }
        syn::visit::visit_expr_method_call(self, c);
    }
}

fn check_stage_writers(files: &[SourceFile], r: &mut Report) {
    const R: &str = "stage-single-writer";
    r.rule(R);
    for f in files {
        let mut v = StageWriters {
            file: &f.path,
            module: f.module.clone(),
            out: Vec::new(),
        };
        v.visit_file(&f.ast);
        for m in v.out {
            r.fail(R, m);
        }
    }
}

// ---------------------------------------------------------------- rejected Smithay APIs

fn check_rejected_apis(crates: &BTreeMap<&str, Vec<SourceFile>>, r: &mut Report) {
    const R: &str = "smithay-rejected-apis";
    r.rule(R);
    for (name, _, _) in &CRATES[..PRODUCT_CRATES] {
        let Some(files) = crates.get(name) else {
            continue;
        };
        for f in files {
            let mut ids = Vec::new();
            idents(f.ast.to_token_stream(), &mut ids);
            for i in ids
                .iter()
                .filter(|i| REJECTED_SMITHAY.iter().any(|x| *i == x))
            {
                r.fail(R, format!("{}: names `{i}` (OB-26)", at(&f.path, i.span())));
            }
        }
    }
}

/// Core's `Effect` values are decisions: product crates other than core may match them but never
/// construct them (INC-3: a fabricated pointer effect would deliver input Core did not route).
/// Renaming or glob/variant imports of `Effect` are refused (fail closed); macro bodies are
/// scanned for `Effect ::`.
fn check_effects_only_from_core(crates: &BTreeMap<&str, Vec<SourceFile>>, r: &mut Report) {
    const R: &str = "effects-only-from-core";
    r.rule(R);
    struct V<'a> {
        file: &'a Path,
        in_pat: usize,
        found: Vec<String>,
    }
    fn effect_variant(p: &syn::Path) -> bool {
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        segs.windows(2).any(|w| w[0] == "Effect")
    }
    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_pat(&mut self, p: &'ast syn::Pat) {
            self.in_pat += 1;
            syn::visit::visit_pat(self, p);
            self.in_pat -= 1;
        }
        fn visit_expr_path(&mut self, e: &'ast syn::ExprPath) {
            if self.in_pat == 0 && effect_variant(&e.path) {
                self.found
                    .push(at(self.file, e.path.segments[0].ident.span()));
            }
            syn::visit::visit_expr_path(self, e);
        }
        fn visit_expr_struct(&mut self, e: &'ast syn::ExprStruct) {
            if effect_variant(&e.path) {
                self.found
                    .push(at(self.file, e.path.segments[0].ident.span()));
            }
            syn::visit::visit_expr_struct(self, e);
        }
        fn visit_use_tree(&mut self, u: &'ast syn::UseTree) {
            match u {
                syn::UseTree::Path(p) if p.ident == "Effect" => {
                    self.found.push(at(self.file, p.ident.span()))
                }
                syn::UseTree::Rename(r) if r.ident == "Effect" => {
                    self.found.push(at(self.file, r.ident.span()))
                }
                _ => {}
            }
            syn::visit::visit_use_tree(self, u);
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            fn scan(ts: TokenStream, v: &mut V<'_>) {
                let toks: Vec<TokenTree> = ts.into_iter().collect();
                for (k, t) in toks.iter().enumerate() {
                    match t {
                        TokenTree::Group(g) => scan(g.stream(), v),
                        TokenTree::Ident(i)
                            if i == "Effect"
                                && matches!(toks.get(k + 1), Some(TokenTree::Punct(p)) if p.as_char() == ':') =>
                        {
                            v.found.push(at(v.file, i.span()))
                        }
                        _ => {}
                    }
                }
            }
            scan(m.tokens.clone(), self);
            syn::visit::visit_macro(self, m);
        }
    }
    for (name, _, _) in CRATES[..PRODUCT_CRATES]
        .iter()
        .filter(|(n, _, _)| *n != "infynitae-core")
    {
        let Some(files) = crates.get(name) else {
            continue;
        };
        for f in files {
            let mut v = V {
                file: &f.path,
                in_pat: 0,
                found: Vec::new(),
            };
            v.visit_file(&f.ast);
            for loc in v.found {
                r.fail(
                    R,
                    format!("{loc}: constructs or re-imports a core `Effect` outside core"),
                );
            }
        }
    }
}

/// Smithay pointer delivery only in `infynitae-wayland`'s `pointer` module (INC-3). Method calls,
/// qualified calls and macro tokens are all checked.
fn check_pointer_delivery(crates: &BTreeMap<&str, Vec<SourceFile>>, r: &mut Report) {
    const R: &str = "smithay-pointer-only-in-adapter-pointer";
    r.rule(R);
    struct V<'a> {
        file: &'a Path,
        found: Vec<(String, String)>,
    }
    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            if POINTER_DELIVERY.iter().any(|n| m.method == n) {
                self.found
                    .push((m.method.to_string(), at(self.file, m.method.span())));
            }
            syn::visit::visit_expr_method_call(self, m);
        }
        fn visit_expr_path(&mut self, e: &'ast syn::ExprPath) {
            if let Some(last) = e.path.segments.last()
                && e.path.segments.len() >= 2
                && POINTER_DELIVERY.iter().any(|n| last.ident == n)
            {
                self.found
                    .push((last.ident.to_string(), at(self.file, last.ident.span())));
            }
            syn::visit::visit_expr_path(self, e);
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            let mut ids = Vec::new();
            idents(m.tokens.clone(), &mut ids);
            for i in ids
                .iter()
                .filter(|i| POINTER_DELIVERY.iter().any(|n| *i == n))
            {
                self.found.push((i.to_string(), at(self.file, i.span())));
            }
            syn::visit::visit_macro(self, m);
        }
    }
    for (name, _, _) in CRATES[..PRODUCT_CRATES]
        .iter()
        .filter(|(n, _, _)| *n != "infynitae-core")
    {
        let Some(files) = crates.get(name) else {
            continue;
        };
        for f in files {
            if *name == "infynitae-wayland" && f.module == ["pointer"] {
                continue;
            }
            let mut v = V {
                file: &f.path,
                found: Vec::new(),
            };
            v.visit_file(&f.ast);
            for (n, loc) in v.found {
                r.fail(
                    R,
                    format!("{loc}: calls `{n}` outside infynitae-wayland::pointer (C4.2)"),
                );
            }
        }
    }
}

fn check_wayland_geometry(files: &[SourceFile], r: &mut Report) {
    const R: &str = "smithay-geometry-only-in-convert";
    r.rule(R);
    for f in files.iter().filter(|f| f.module != ["convert"]) {
        let mut ids = Vec::new();
        idents(f.ast.to_token_stream(), &mut ids);
        for i in ids
            .iter()
            .filter(|i| SMITHAY_GEOMETRY.iter().any(|g| *i == g))
        {
            r.fail(
                R,
                format!(
                    "{}: names `{i}` outside `convert` (INV-COORD-4)",
                    at(&f.path, i.span())
                ),
            );
        }
    }
}

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
        // integration tests, benches, examples and build scripts of the crate are code too
        let crate_dir = root_file
            .path
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."));
        let mut extra = Vec::new();
        for sub in ["tests", "benches", "examples"] {
            rust_files(&crate_dir.join(sub), &mut extra);
        }
        if crate_dir.join("build.rs").exists() {
            extra.push(crate_dir.join("build.rs"));
        }
        let mut extra_files = Vec::new();
        for path in extra {
            match parse(&path) {
                Ok(ast) => extra_files.push(SourceFile {
                    path,
                    module: vec![],
                    ast,
                }),
                Err(e) => r.fail(R, format!("unparsable file (fail closed): {e}")),
            }
        }
        for f in files.iter().chain(&extra_files) {
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

/// Every `.rs` file under `dir`, recursively, in a stable order.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
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
