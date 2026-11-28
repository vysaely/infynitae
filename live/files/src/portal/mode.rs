//! Pure state machine for the FileChooser portal picker (OpenFile, SaveFile, SaveFiles).
//! Carries no Wayland or Cairo types so every keyboard flow, filter, path-typing edit,
//! overwrite check, and Escape priority rule is unit-tested directly without a display.

#[cfg(test)]
use crate::fs_list::list_directory_sync;
use crate::fs_list::{
    entry_from_recent, shorten_home, sort_entries, FileEntry, SortColumn, SortOrder,
};
use crate::mime::MimeDb;
use crate::places::{home_dir, Places};
use crate::portal::filters::FilterSet;
use crate::portal::protocol::{
    collision_rename, decode_dbus_bytes, decode_dbus_path, path_exists_any, path_to_file_uri,
    sanitize_save_name, split_current_file, strip_mnemonic, PortalChoice, PortalRequest,
    PortalResponse,
};
use crate::preview::PreviewContent;
use crate::recent::{load_recent_files, record_recent_files, xbel_path, RecentItem};
use crate::text_input::{complete_path, normalize_lexical, resolve_typed_path, TextInput};
use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerMode {
    OpenFile { multiple: bool, directory: bool },
    SaveFile,
    SaveFiles { files: Vec<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Places,
    PathEdit,
    Search,
    NewFolder,
    FileList,
    SaveName,
    FilterBtn,
    CancelBtn,
    AcceptBtn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMenu {
    Filter,
    Choice(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverwritePrompt {
    Single {
        target: PathBuf,
        name: String,
    },
    Multi {
        dir: PathBuf,
        colliding: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceTarget {
    Recent,
    Folder(usize),
    Drive(usize),
}

/// Platform-independent key input passed into `PickerState::handle_key`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Space,
    Char(char),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Side effects requested by the state machine and performed by the Wayland runner (or asserted in tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerEffect {
    LoadDir(PathBuf),
    LoadPreview(PathBuf),
    ClearPreview,
    SetClipboard(String),
    Finish(PortalResponse),
}

pub struct PickerState {
    pub mode: PickerMode,
    pub app_id: String,
    pub parent_window: String,
    pub title: String,
    pub accept_label: String,
    pub home: Option<PathBuf>,
    pub places: Places,
    pub mime_db: Arc<MimeDb>,
    pub filters: FilterSet,
    pub choices: Vec<PortalChoice>,
    pub choice_values: HashMap<String, String>,

    pub current_dir: PathBuf,
    pub in_recent_view: bool,
    pub back_stack: Vec<Location>,
    pub forward_stack: Vec<Location>,
    pub loading: bool,
    pub raw_entries: Vec<FileEntry>,
    pub visible: Vec<usize>,
    pub cursor: Option<usize>,
    pub selection_anchor: Option<usize>,
    pub selected: BTreeSet<usize>,
    pub sort_col: SortColumn,
    pub sort_order: SortOrder,
    pub show_hidden: bool,
    pub list_scroll: f64,
    pub places_scroll: f64,
    pub place_cursor: usize,

    pub focus: Focus,
    pub path_editing: bool,
    pub path_input: TextInput,
    pub search_input: TextInput,
    pub new_folder: Option<TextInput>,
    pub save_input: TextInput,
    pub open_menu: Option<OpenMenu>,
    pub overwrite: Option<OverwritePrompt>,
    pub error_msg: Option<String>,

    pub preview_open: bool,
    pub preview: PreviewContent,
    pub pending_select_name: Option<String>,
    pub xbel_override: Option<PathBuf>,
    pub recent_items: Vec<RecentItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Dir(PathBuf),
    Recent,
}

impl PickerState {
    pub fn from_request(
        req: PortalRequest,
        places: Places,
        mime_db: Arc<MimeDb>,
        home: Option<PathBuf>,
        recent_items: Vec<RecentItem>,
    ) -> (Self, Vec<PickerEffect>) {
        let multiple = req.options.multiple.unwrap_or(false);
        let directory = req.options.directory.unwrap_or(false);

        let mode = match req.method.as_str() {
            "SaveFile" => PickerMode::SaveFile,
            "SaveFiles" => {
                let files: Vec<String> = req
                    .options
                    .files_b64
                    .iter()
                    .filter_map(|b64| decode_dbus_bytes(b64))
                    .map(|raw| sanitize_save_name(&String::from_utf8_lossy(&raw)))
                    .filter(|s| !s.is_empty())
                    .collect();
                PickerMode::SaveFiles { files }
            }
            _ => PickerMode::OpenFile {
                multiple,
                directory,
            },
        };

        let default_title = match &mode {
            PickerMode::OpenFile {
                directory: true, ..
            } => "Select folder",
            PickerMode::OpenFile { .. } => "Open file",
            PickerMode::SaveFile => "Save file",
            PickerMode::SaveFiles { .. } => "Save files",
        };
        let title = if req.title.trim().is_empty() {
            default_title.to_string()
        } else {
            req.title.clone()
        };

        let default_accept = match &mode {
            PickerMode::OpenFile {
                directory: true, ..
            } => "Select folder",
            PickerMode::OpenFile { .. } => "Open",
            PickerMode::SaveFile | PickerMode::SaveFiles { .. } => "Save",
        };
        let accept_label = req
            .options
            .accept_label
            .as_deref()
            .map(strip_mnemonic)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| default_accept.to_string());

        let mut start_dir = req
            .options
            .current_folder_b64
            .as_deref()
            .and_then(decode_dbus_path)
            .filter(|p| p.is_dir());

        let mut initial_save_name = req
            .options
            .current_name
            .as_deref()
            .map(sanitize_save_name)
            .unwrap_or_default();

        if matches!(mode, PickerMode::SaveFile) {
            if let Some(raw_cf) = req
                .options
                .current_file_b64
                .as_deref()
                .and_then(decode_dbus_bytes)
            {
                let (cf_dir, cf_name) = split_current_file(&raw_cf);
                if let Some(d) = cf_dir.filter(|p| p.is_dir()) {
                    start_dir = Some(d);
                }
                if let Some(n) = cf_name {
                    initial_save_name = n;
                }
            }
        }

        let current_dir = start_dir
            .or_else(|| home.clone().filter(|p| p.is_dir()))
            .unwrap_or_else(|| PathBuf::from("/"));

        let filters = FilterSet::from_request(req.options.filters, req.options.current_filter);

        let mut choice_values = HashMap::new();
        for ch in &req.options.choices {
            let val = if !ch.initial.is_empty() {
                ch.initial.clone()
            } else if ch.options.is_empty() {
                "false".to_string()
            } else {
                ch.options[0].id.clone()
            };
            choice_values.insert(ch.id.clone(), val);
        }

        let mut save_input = TextInput::new(initial_save_name);
        save_input.select_stem_before_dot();

        let focus = if matches!(mode, PickerMode::SaveFile) {
            Focus::SaveName
        } else {
            Focus::FileList
        };

        let mut state = Self {
            mode,
            app_id: req.app_id,
            parent_window: req.parent_window,
            title,
            accept_label,
            home,
            places,
            mime_db,
            filters,
            choices: req.options.choices,
            choice_values,
            current_dir: current_dir.clone(),
            in_recent_view: false,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            loading: true,
            raw_entries: Vec::new(),
            visible: Vec::new(),
            cursor: None,
            selection_anchor: None,
            selected: BTreeSet::new(),
            sort_col: SortColumn::Name,
            sort_order: SortOrder::Asc,
            show_hidden: false,
            list_scroll: 0.0,
            places_scroll: 0.0,
            place_cursor: 0,
            focus,
            path_editing: false,
            path_input: TextInput::new(""),
            search_input: TextInput::new(""),
            new_folder: None,
            save_input,
            open_menu: None,
            overwrite: None,
            error_msg: None,
            preview_open: true,
            preview: PreviewContent::Empty,
            pending_select_name: None,
            xbel_override: None,
            recent_items,
        };

        let effects = vec![PickerEffect::LoadDir(current_dir)];
        state.sync_path_input();
        (state, effects)
    }

    pub fn from_system(req: PortalRequest) -> (Self, Vec<PickerEffect>) {
        let home = home_dir();
        let places = Places::load_system();
        let mime_db = Arc::new(MimeDb::load_system());
        let recent = load_recent_files();
        Self::from_request(req, places, mime_db, home, recent)
    }

    pub fn is_multiple(&self) -> bool {
        matches!(self.mode, PickerMode::OpenFile { multiple: true, .. })
    }

    pub fn is_directory_mode(&self) -> bool {
        matches!(
            self.mode,
            PickerMode::OpenFile {
                directory: true,
                ..
            }
        )
    }

    pub fn can_go_back(&self) -> bool {
        !self.back_stack.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward_stack.is_empty()
    }

    pub fn can_go_up(&self) -> bool {
        !self.in_recent_view && self.current_dir.parent().is_some()
    }

    /// Breadcrumb segments `(label, full_path)` for the top path bar.
    pub fn breadcrumbs(&self) -> Vec<(String, PathBuf)> {
        if self.in_recent_view {
            return vec![("Recent".to_string(), PathBuf::new())];
        }
        let mut out = Vec::new();
        if let Some(ref h) = self.home {
            if self.current_dir == *h {
                return vec![("Home".to_string(), h.clone())];
            }
            if let Ok(rel) = self.current_dir.strip_prefix(h) {
                out.push(("Home".to_string(), h.clone()));
                let mut acc = h.clone();
                for comp in rel.components() {
                    acc.push(comp);
                    out.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
                }
                return out;
            }
        }
        // Check if current_dir sits on a named Roman drive (e.g. /mnt/III -> "III").
        for d in &self.places.drives {
            if d.mount_point != Path::new("/") {
                if self.current_dir == d.mount_point {
                    return vec![(d.name.clone(), d.mount_point.clone())];
                }
                if let Ok(rel) = self.current_dir.strip_prefix(&d.mount_point) {
                    out.push((d.name.clone(), d.mount_point.clone()));
                    let mut acc = d.mount_point.clone();
                    for comp in rel.components() {
                        acc.push(comp);
                        out.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
                    }
                    return out;
                }
            }
        }
        out.push(("/".to_string(), PathBuf::from("/")));
        let mut acc = PathBuf::from("/");
        for comp in self.current_dir.components().skip(1) {
            acc.push(comp);
            out.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
        }
        out
    }

    /// Whether the primary action button (`Open` / `Save` / `Select folder`) should be enabled.
    pub fn can_accept(&self) -> bool {
        match &self.mode {
            PickerMode::OpenFile {
                directory: true, ..
            } => !self.in_recent_view,
            PickerMode::OpenFile { .. } => self.selected_entries().iter().any(|e| !e.is_dir),
            PickerMode::SaveFile => {
                !self.in_recent_view && !self.save_input.as_str().trim().is_empty()
            }
            PickerMode::SaveFiles { files } => !self.in_recent_view && !files.is_empty(),
        }
    }

    pub fn selected_entries(&self) -> Vec<&FileEntry> {
        let mut out = Vec::new();
        for &vis_idx in &self.selected {
            if let Some(&raw_idx) = self.visible.get(vis_idx) {
                if let Some(e) = self.raw_entries.get(raw_idx) {
                    out.push(e);
                }
            }
        }
        if out.is_empty() {
            if let Some(c) = self.cursor {
                if let Some(&raw_idx) = self.visible.get(c) {
                    if let Some(e) = self.raw_entries.get(raw_idx) {
                        out.push(e);
                    }
                }
            }
        }
        out
    }

    pub fn visible_entry(&self, vis_idx: usize) -> Option<&FileEntry> {
        let raw_idx = *self.visible.get(vis_idx)?;
        self.raw_entries.get(raw_idx)
    }

    /// Applies an asynchronously loaded directory listing.
    pub fn apply_dir_loaded(
        &mut self,
        dir: PathBuf,
        result: Result<Vec<FileEntry>, String>,
    ) -> Vec<PickerEffect> {
        if self.in_recent_view || dir != self.current_dir {
            return Vec::new();
        }
        self.loading = false;
        match result {
            Ok(mut entries) => {
                sort_entries(&mut entries, self.sort_col, self.sort_order);
                self.raw_entries = entries;
                self.error_msg = None;
            }
            Err(msg) => {
                self.raw_entries.clear();
                self.error_msg = Some(msg);
            }
        }
        self.rebuild_visible();
        if let Some(target_name) = self.pending_select_name.take() {
            if let Some(vis_idx) = self
                .visible
                .iter()
                .position(|&ri| self.raw_entries[ri].display_name == target_name)
            {
                self.cursor = Some(vis_idx);
                self.selected.clear();
                self.selected.insert(vis_idx);
                self.selection_anchor = Some(vis_idx);
            }
        }
        self.refresh_preview_effect()
    }

    /// Helper for synchronous tests without spawning `DirWorker`.
    #[cfg(test)]
    pub fn load_current_dir_sync(&mut self) -> Vec<PickerEffect> {
        let dir = self.current_dir.clone();
        let res = list_directory_sync(&dir, &self.mime_db);
        self.apply_dir_loaded(dir, res)
    }

    pub fn apply_preview_loaded(&mut self, content: PreviewContent) {
        self.preview = content;
    }

    pub fn rebuild_visible(&mut self) {
        let q = self.search_input.as_str().trim().to_ascii_lowercase();
        let dir_only = self.is_directory_mode();

        let prev_selected_paths: Vec<PathBuf> = self
            .selected
            .iter()
            .filter_map(|&vi| {
                self.visible
                    .get(vi)
                    .and_then(|&ri| self.raw_entries.get(ri))
            })
            .map(|e| e.path.clone())
            .collect();

        self.visible.clear();
        for (i, e) in self.raw_entries.iter().enumerate() {
            if !self.show_hidden && e.is_hidden {
                continue;
            }
            if dir_only && !e.is_dir {
                continue;
            }
            if !self
                .filters
                .matches_entry(&e.display_name, &e.mime, e.is_dir, &self.mime_db)
            {
                continue;
            }
            if !q.is_empty() && !e.display_name.to_ascii_lowercase().contains(&q) {
                continue;
            }
            self.visible.push(i);
        }

        self.selected.clear();
        for (vi, &ri) in self.visible.iter().enumerate() {
            if prev_selected_paths.contains(&self.raw_entries[ri].path) {
                self.selected.insert(vi);
            }
        }
        if let Some(c) = self.cursor {
            if c >= self.visible.len() {
                self.cursor = self.visible.len().checked_sub(1);
            }
        }
        if self.cursor.is_none()
            && !self.visible.is_empty()
            && !matches!(self.mode, PickerMode::SaveFile)
        {
            self.cursor = Some(0);
        }
        self.list_scroll = 0.0;
    }

    fn current_location(&self) -> Location {
        if self.in_recent_view {
            Location::Recent
        } else {
            Location::Dir(self.current_dir.clone())
        }
    }

    pub fn navigate_to(&mut self, dir: PathBuf) -> Vec<PickerEffect> {
        let norm = normalize_lexical(&dir);
        if !self.in_recent_view && norm == self.current_dir && !self.loading {
            return Vec::new();
        }
        self.back_stack.push(self.current_location());
        self.forward_stack.clear();
        self.enter_location(Location::Dir(norm))
    }

    pub fn open_recent_view(&mut self) -> Vec<PickerEffect> {
        if self.in_recent_view {
            return Vec::new();
        }
        self.back_stack.push(self.current_location());
        self.forward_stack.clear();
        self.enter_location(Location::Recent)
    }

    pub fn open_place(&mut self, target: PlaceTarget) -> Vec<PickerEffect> {
        self.open_menu = None;
        match target {
            PlaceTarget::Recent => self.open_recent_view(),
            PlaceTarget::Folder(i) => {
                if let Some(f) = self.places.folders.get(i) {
                    self.navigate_to(f.path.clone())
                } else {
                    Vec::new()
                }
            }
            PlaceTarget::Drive(i) => {
                if let Some(d) = self.places.drives.get(i) {
                    self.navigate_to(d.mount_point.clone())
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub fn go_back(&mut self) -> Vec<PickerEffect> {
        let Some(prev) = self.back_stack.pop() else {
            return Vec::new();
        };
        self.forward_stack.push(self.current_location());
        self.enter_location(prev)
    }

    pub fn go_forward(&mut self) -> Vec<PickerEffect> {
        let Some(next) = self.forward_stack.pop() else {
            return Vec::new();
        };
        self.back_stack.push(self.current_location());
        self.enter_location(next)
    }

    pub fn go_up(&mut self) -> Vec<PickerEffect> {
        if self.in_recent_view {
            return Vec::new();
        }
        let Some(parent) = self.current_dir.parent().map(Path::to_path_buf) else {
            return Vec::new();
        };
        self.navigate_to(parent)
    }

    fn enter_location(&mut self, loc: Location) -> Vec<PickerEffect> {
        self.overwrite = None;
        self.new_folder = None;
        self.error_msg = None;
        self.open_menu = None;
        self.path_editing = false;
        self.search_input.clear();
        self.selected.clear();
        self.cursor = None;
        self.selection_anchor = None;
        self.list_scroll = 0.0;

        match loc {
            Location::Recent => {
                self.in_recent_view = true;
                self.loading = false;
                self.raw_entries = self
                    .recent_items
                    .iter()
                    .filter_map(|item| {
                        entry_from_recent(
                            item.path.clone(),
                            self.home.as_deref(),
                            &self.mime_db,
                            item.modified_secs,
                        )
                    })
                    .collect();
                self.rebuild_visible();
                self.refresh_preview_effect()
            }
            Location::Dir(dir) => {
                self.in_recent_view = false;
                self.current_dir = dir.clone();
                self.loading = true;
                self.raw_entries.clear();
                self.visible.clear();
                self.sync_path_input();
                let mut fx = vec![PickerEffect::ClearPreview, PickerEffect::LoadDir(dir)];
                self.preview = PreviewContent::Empty;
                if let Some(p) = self.selected_preview_effect() {
                    fx.push(p);
                }
                fx
            }
        }
    }

    fn sync_path_input(&mut self) {
        let s = shorten_home(&self.current_dir, self.home.as_deref());
        self.path_input.set_text(s);
    }

    pub fn start_path_edit(&mut self) {
        if self.in_recent_view {
            self.path_input
                .set_text(shorten_home(&self.current_dir, self.home.as_deref()));
        } else {
            self.sync_path_input();
        }
        self.path_input.select_all();
        self.path_editing = true;
        self.open_menu = None;
        self.focus = Focus::PathEdit;
    }

    pub fn toggle_hidden(&mut self) -> Vec<PickerEffect> {
        self.show_hidden = !self.show_hidden;
        self.rebuild_visible();
        self.refresh_preview_effect()
    }

    pub fn toggle_preview(&mut self) -> Vec<PickerEffect> {
        self.preview_open = !self.preview_open;
        if self.preview_open {
            self.refresh_preview_effect()
        } else {
            vec![PickerEffect::ClearPreview]
        }
    }

    pub fn start_new_folder(&mut self) {
        if self.in_recent_view {
            self.error_msg = Some("Switch to a folder first to create a new folder".to_string());
            return;
        }
        self.open_menu = None;
        self.overwrite = None;
        self.error_msg = None;
        self.new_folder = Some(TextInput::new(""));
        self.focus = Focus::NewFolder;
    }

    pub fn confirm_new_folder(&mut self) -> Vec<PickerEffect> {
        let Some(ref input) = self.new_folder else {
            return Vec::new();
        };
        let name = input.as_str().trim().to_string();
        if name.is_empty() {
            self.error_msg = Some("Enter a folder name".to_string());
            return Vec::new();
        }
        if name == "." || name == ".." || name.contains('/') || name.contains('\0') {
            self.error_msg = Some("Folder name cannot contain '/' or be '.' / '..'".to_string());
            return Vec::new();
        }
        let target = self.current_dir.join(&name);
        match fs::create_dir(&target) {
            Ok(()) => {
                self.new_folder = None;
                self.error_msg = None;
                self.focus = Focus::FileList;
                self.pending_select_name = Some(name);
                self.loading = true;
                vec![PickerEffect::LoadDir(self.current_dir.clone())]
            }
            Err(e) => {
                self.error_msg = Some(format!("Could not create folder: {e}"));
                Vec::new()
            }
        }
    }

    pub fn set_sort(&mut self, col: SortColumn) -> Vec<PickerEffect> {
        if self.sort_col == col {
            self.sort_order = self.sort_order.flip();
        } else {
            self.sort_col = col;
            self.sort_order = match col {
                SortColumn::Name => SortOrder::Asc,
                SortColumn::Modified | SortColumn::Size => SortOrder::Desc,
            };
        }
        if !self.in_recent_view {
            sort_entries(&mut self.raw_entries, self.sort_col, self.sort_order);
        }
        self.rebuild_visible();
        self.refresh_preview_effect()
    }

    pub fn select_filter(&mut self, index: usize) -> Vec<PickerEffect> {
        self.open_menu = None;
        if self.filters.set_active(index) {
            self.rebuild_visible();
            return self.refresh_preview_effect();
        }
        Vec::new()
    }

    pub fn toggle_bool_choice(&mut self, idx: usize) {
        if let Some(ch) = self.choices.get(idx) {
            let cur = self
                .choice_values
                .get(&ch.id)
                .map(String::as_str)
                .unwrap_or("false");
            let next = if cur == "true" { "false" } else { "true" };
            self.choice_values.insert(ch.id.clone(), next.to_string());
        }
    }

    pub fn set_choice_option(&mut self, choice_idx: usize, opt_id: String) {
        self.open_menu = None;
        if let Some(ch) = self.choices.get(choice_idx) {
            self.choice_values.insert(ch.id.clone(), opt_id);
        }
    }

    pub fn click_row(&mut self, vis_idx: usize, mods: KeyMods) -> Vec<PickerEffect> {
        if vis_idx >= self.visible.len() {
            return Vec::new();
        }
        self.open_menu = None;
        self.overwrite = None;
        self.error_msg = None;
        self.focus = Focus::FileList;

        if self.is_multiple() && mods.shift {
            let anchor = self.selection_anchor.unwrap_or(vis_idx);
            let (lo, hi) = if anchor <= vis_idx {
                (anchor, vis_idx)
            } else {
                (vis_idx, anchor)
            };
            self.selected.clear();
            for i in lo..=hi {
                self.selected.insert(i);
            }
            self.cursor = Some(vis_idx);
        } else if self.is_multiple() && mods.ctrl {
            if !self.selected.remove(&vis_idx) {
                self.selected.insert(vis_idx);
            }
            self.cursor = Some(vis_idx);
            self.selection_anchor = Some(vis_idx);
        } else {
            self.selected.clear();
            self.selected.insert(vis_idx);
            self.cursor = Some(vis_idx);
            self.selection_anchor = Some(vis_idx);
        }

        if matches!(self.mode, PickerMode::SaveFile) {
            if let Some(e) = self.visible_entry(vis_idx) {
                if !e.is_dir {
                    let name = e.display_name.clone();
                    self.save_input.set_text(name);
                }
            }
        }
        self.refresh_preview_effect()
    }

    pub fn activate_row(&mut self, vis_idx: usize) -> Vec<PickerEffect> {
        let Some(entry) = self.visible_entry(vis_idx).cloned() else {
            return Vec::new();
        };
        if entry.is_dir {
            return self.navigate_to(entry.path);
        }
        self.selected.clear();
        self.selected.insert(vis_idx);
        self.cursor = Some(vis_idx);
        if matches!(self.mode, PickerMode::SaveFile) {
            self.save_input.set_text(entry.display_name);
        }
        self.try_accept()
    }

    pub fn try_accept(&mut self) -> Vec<PickerEffect> {
        self.open_menu = None;
        self.error_msg = None;

        match self.mode.clone() {
            PickerMode::OpenFile {
                directory: true, ..
            } => {
                if self.in_recent_view {
                    return Vec::new();
                }
                let chosen: Vec<PathBuf> = {
                    let sel: Vec<PathBuf> = self
                        .selected_entries()
                        .into_iter()
                        .filter(|e| e.is_dir)
                        .map(|e| e.path.clone())
                        .collect();
                    if sel.is_empty() {
                        vec![self.current_dir.clone()]
                    } else if self.is_multiple() {
                        sel
                    } else {
                        vec![sel[0].clone()]
                    }
                };
                self.finish_with_paths(chosen, true)
            }
            PickerMode::OpenFile { multiple, .. } => {
                let entries = self.selected_entries();
                if entries.is_empty() {
                    return Vec::new();
                }
                if entries.len() == 1 && entries[0].is_dir {
                    return self.navigate_to(entries[0].path.clone());
                }
                let files: Vec<PathBuf> = entries
                    .into_iter()
                    .filter(|e| !e.is_dir)
                    .map(|e| e.path.clone())
                    .collect();
                if files.is_empty() {
                    return Vec::new();
                }
                let out = if multiple {
                    files
                } else {
                    vec![files[0].clone()]
                };
                self.finish_with_paths(out, true)
            }
            PickerMode::SaveFile => {
                if self.in_recent_view {
                    self.error_msg = Some("Choose a folder to save into".to_string());
                    return Vec::new();
                }
                let raw_name = self.save_input.as_str().trim();
                let name = sanitize_save_name(raw_name);
                if name.is_empty() || name == "." || name == ".." {
                    self.error_msg = Some("Enter a valid file name".to_string());
                    return Vec::new();
                }
                if !is_dir_writable(&self.current_dir) {
                    self.error_msg = Some(format!(
                        "Cannot save in {}: folder is not writable",
                        self.current_dir.display()
                    ));
                    return Vec::new();
                }
                let target = normalize_lexical(&self.current_dir.join(&name));
                if path_exists_any(&target) {
                    if target.is_dir() {
                        return self.navigate_to(target);
                    }
                    self.overwrite = Some(OverwritePrompt::Single { target, name });
                    return Vec::new();
                }
                self.finish_with_paths(vec![target], false)
            }
            PickerMode::SaveFiles { files } => {
                if self.in_recent_view || files.is_empty() {
                    return Vec::new();
                }
                if !is_dir_writable(&self.current_dir) {
                    self.error_msg = Some(format!(
                        "Cannot save in {}: folder is not writable",
                        self.current_dir.display()
                    ));
                    return Vec::new();
                }
                let colliding: Vec<String> = files
                    .iter()
                    .filter(|f| path_exists_any(&self.current_dir.join(f)))
                    .cloned()
                    .collect();
                if !colliding.is_empty() {
                    self.overwrite = Some(OverwritePrompt::Multi {
                        dir: self.current_dir.clone(),
                        colliding,
                    });
                    return Vec::new();
                }
                let paths: Vec<PathBuf> = files
                    .iter()
                    .map(|f| normalize_lexical(&self.current_dir.join(f)))
                    .collect();
                self.finish_with_paths(paths, false)
            }
        }
    }

    pub fn confirm_overwrite_replace(&mut self) -> Vec<PickerEffect> {
        let Some(prompt) = self.overwrite.take() else {
            return Vec::new();
        };
        match prompt {
            OverwritePrompt::Single { target, .. } => self.finish_with_paths(vec![target], false),
            OverwritePrompt::Multi { dir, .. } => {
                let PickerMode::SaveFiles { files } = &self.mode else {
                    return Vec::new();
                };
                let paths = files
                    .iter()
                    .map(|f| normalize_lexical(&dir.join(f)))
                    .collect();
                self.finish_with_paths(paths, false)
            }
        }
    }

    pub fn confirm_overwrite_rename_colliding(&mut self) -> Vec<PickerEffect> {
        let Some(OverwritePrompt::Multi { dir, .. }) = self.overwrite.take() else {
            return Vec::new();
        };
        let PickerMode::SaveFiles { files } = &self.mode else {
            return Vec::new();
        };
        let paths = files
            .iter()
            .map(|f| {
                let unique = collision_rename(&dir, f);
                normalize_lexical(&dir.join(unique))
            })
            .collect();
        self.finish_with_paths(paths, false)
    }

    pub fn dismiss_overwrite(&mut self) {
        self.overwrite = None;
    }

    fn finish_with_paths(&mut self, paths: Vec<PathBuf>, is_open: bool) -> Vec<PickerEffect> {
        if let Some(xbel) = self.xbel_override.clone().or_else(xbel_path) {
            let _ = record_recent_files(&xbel, &paths, &self.mime_db, &self.app_id);
        }
        let uris: Vec<String> = paths.iter().map(|p| path_to_file_uri(p)).collect();
        let choices = if self.choices.is_empty() {
            None
        } else {
            Some(
                self.choices
                    .iter()
                    .map(|c| {
                        let v = self.choice_values.get(&c.id).cloned().unwrap_or_default();
                        (c.id.clone(), v)
                    })
                    .collect(),
            )
        };
        let current_filter = self.filters.active_filter().cloned();
        let resp = PortalResponse {
            response: 0,
            uris,
            writable: is_open.then_some(true),
            choices,
            current_filter,
        };
        vec![PickerEffect::Finish(resp)]
    }

    pub fn cancel(&mut self) -> Vec<PickerEffect> {
        vec![PickerEffect::Finish(PortalResponse::cancelled())]
    }

    pub fn handle_paste(&mut self, text: &str) -> Vec<PickerEffect> {
        let changed = match self.focus {
            Focus::PathEdit if self.path_editing => self.path_input.insert(text),
            Focus::Search => {
                if self.search_input.insert(text) {
                    self.rebuild_visible();
                    return self.refresh_preview_effect();
                }
                false
            }
            Focus::NewFolder => self.new_folder.as_mut().is_some_and(|ti| ti.insert(text)),
            Focus::SaveName => {
                self.overwrite = None;
                self.save_input.insert(text)
            }
            _ => false,
        };
        let _ = changed;
        Vec::new()
    }

    pub fn handle_key(&mut self, key: Key, mods: KeyMods) -> Vec<PickerEffect> {
        // Escape follows strict priority order (docs/FILE_PICKER.md).
        if key == Key::Escape {
            if self.open_menu.is_some() {
                self.open_menu = None;
                return Vec::new();
            }
            if self.overwrite.is_some() {
                self.overwrite = None;
                return Vec::new();
            }
            if self.new_folder.is_some() {
                self.new_folder = None;
                self.error_msg = None;
                self.focus = Focus::FileList;
                return Vec::new();
            }
            if self.path_editing {
                self.path_editing = false;
                self.error_msg = None;
                self.sync_path_input();
                self.focus = Focus::FileList;
                return Vec::new();
            }
            if !self.search_input.is_empty() || self.focus == Focus::Search {
                self.search_input.clear();
                self.focus = Focus::FileList;
                self.rebuild_visible();
                return self.refresh_preview_effect();
            }
            return self.cancel();
        }

        // Global shortcuts.
        if mods.ctrl && !mods.alt {
            match key {
                Key::Char('l') | Key::Char('L') => {
                    self.start_path_edit();
                    return Vec::new();
                }
                Key::Char('f') | Key::Char('F') => {
                    self.open_menu = None;
                    self.path_editing = false;
                    self.focus = Focus::Search;
                    self.search_input.select_all();
                    return Vec::new();
                }
                Key::Char('h') | Key::Char('H') => {
                    return self.toggle_hidden();
                }
                Key::Char('n') | Key::Char('N') if mods.shift => {
                    self.start_new_folder();
                    return Vec::new();
                }
                _ => {}
            }
        }
        if mods.alt && !mods.ctrl {
            match key {
                Key::Left => return self.go_back(),
                Key::Right => return self.go_forward(),
                Key::Up => return self.go_up(),
                Key::Char('p') | Key::Char('P') => return self.toggle_preview(),
                _ => {}
            }
        }

        // Text-field focused handling.
        if self.focus == Focus::PathEdit && self.path_editing {
            return self.handle_path_edit_key(key, mods);
        }
        if self.focus == Focus::NewFolder && self.new_folder.is_some() {
            return self.handle_new_folder_key(key, mods);
        }
        if self.focus == Focus::Search {
            return self.handle_search_key(key, mods);
        }
        if self.focus == Focus::SaveName {
            return self.handle_save_name_key(key, mods);
        }

        // Tab cycles focus across main regions.
        if key == Key::Tab {
            self.cycle_focus(mods.shift);
            return Vec::new();
        }

        match self.focus {
            Focus::Places => self.handle_places_key(key),
            Focus::FileList => self.handle_list_key(key, mods),
            Focus::FilterBtn => {
                if matches!(key, Key::Enter | Key::Space) && !self.filters.is_empty() {
                    self.open_menu = match self.open_menu {
                        Some(OpenMenu::Filter) => None,
                        _ => Some(OpenMenu::Filter),
                    };
                }
                Vec::new()
            }
            Focus::CancelBtn => {
                if matches!(key, Key::Enter | Key::Space) {
                    return self.cancel();
                }
                Vec::new()
            }
            Focus::AcceptBtn => {
                if matches!(key, Key::Enter | Key::Space) {
                    return self.try_accept();
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_path_edit_key(&mut self, key: Key, mods: KeyMods) -> Vec<PickerEffect> {
        match key {
            Key::Tab => {
                if let Some(completed) = complete_path(
                    self.path_input.as_str(),
                    &self.current_dir,
                    self.home.as_deref(),
                ) {
                    self.path_input.set_text(completed);
                }
                Vec::new()
            }
            Key::Enter => {
                let raw = self.path_input.as_str().trim().to_string();
                let resolved = resolve_typed_path(&raw, &self.current_dir, self.home.as_deref());
                if resolved.is_dir() {
                    self.path_editing = false;
                    self.focus = Focus::FileList;
                    return self.navigate_to(resolved);
                }
                if resolved.is_file() && !self.is_directory_mode() {
                    if let (Some(parent), Some(fname)) = (resolved.parent(), resolved.file_name()) {
                        let fname_str = fname.to_string_lossy().into_owned();
                        self.path_editing = false;
                        if matches!(self.mode, PickerMode::SaveFile) {
                            self.save_input.set_text(fname_str.clone());
                            self.focus = Focus::SaveName;
                        } else {
                            self.focus = Focus::FileList;
                        }
                        self.pending_select_name = Some(fname_str);
                        if parent == self.current_dir && !self.in_recent_view {
                            if let Some(vi) = self
                                .visible
                                .iter()
                                .position(|&ri| self.raw_entries[ri].path == resolved)
                            {
                                self.cursor = Some(vi);
                                self.selected.clear();
                                self.selected.insert(vi);
                                self.pending_select_name = None;
                                return self.refresh_preview_effect();
                            }
                        }
                        return self.navigate_to(parent.to_path_buf());
                    }
                }
                if matches!(self.mode, PickerMode::SaveFile) {
                    if let (Some(parent), Some(fname)) = (resolved.parent(), resolved.file_name()) {
                        if parent.is_dir() {
                            self.path_editing = false;
                            self.save_input
                                .set_text(sanitize_save_name(&fname.to_string_lossy()));
                            self.focus = Focus::SaveName;
                            return self.navigate_to(parent.to_path_buf());
                        }
                    }
                }
                self.error_msg = Some(format!("Folder does not exist: {}", resolved.display()));
                Vec::new()
            }
            other => edit_text_input(&mut self.path_input, other, mods),
        }
    }

    fn handle_new_folder_key(&mut self, key: Key, mods: KeyMods) -> Vec<PickerEffect> {
        match key {
            Key::Enter => self.confirm_new_folder(),
            other => {
                if let Some(ref mut ti) = self.new_folder {
                    edit_text_input(ti, other, mods)
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn handle_search_key(&mut self, key: Key, mods: KeyMods) -> Vec<PickerEffect> {
        match key {
            Key::Tab => {
                self.cycle_focus(mods.shift);
                Vec::new()
            }
            Key::Enter | Key::Down => {
                self.focus = Focus::FileList;
                if self.cursor.is_none() && !self.visible.is_empty() {
                    self.cursor = Some(0);
                    self.selected.clear();
                    self.selected.insert(0);
                }
                self.refresh_preview_effect()
            }
            other => {
                let before = self.search_input.as_str().to_string();
                let fx = edit_text_input(&mut self.search_input, other, mods);
                if self.search_input.as_str() != before {
                    self.rebuild_visible();
                    let mut out = fx;
                    out.extend(self.refresh_preview_effect());
                    return out;
                }
                fx
            }
        }
    }

    fn handle_save_name_key(&mut self, key: Key, mods: KeyMods) -> Vec<PickerEffect> {
        match key {
            Key::Tab => {
                self.cycle_focus(mods.shift);
                Vec::new()
            }
            Key::Enter => {
                if self.overwrite.is_some() {
                    return Vec::new();
                }
                self.try_accept()
            }
            other => {
                let before = self.save_input.as_str().to_string();
                let fx = edit_text_input(&mut self.save_input, other, mods);
                if self.save_input.as_str() != before {
                    self.overwrite = None;
                    self.error_msg = None;
                }
                fx
            }
        }
    }

    fn handle_places_key(&mut self, key: Key) -> Vec<PickerEffect> {
        let total = 1 + self.places.folders.len() + self.places.drives.len();
        if total == 0 {
            return Vec::new();
        }
        match key {
            Key::Up => {
                self.place_cursor = self.place_cursor.saturating_sub(1);
                Vec::new()
            }
            Key::Down => {
                if self.place_cursor + 1 < total {
                    self.place_cursor += 1;
                }
                Vec::new()
            }
            Key::Enter | Key::Space => {
                let target = if self.place_cursor == 0 {
                    PlaceTarget::Recent
                } else if self.place_cursor <= self.places.folders.len() {
                    PlaceTarget::Folder(self.place_cursor - 1)
                } else {
                    PlaceTarget::Drive(self.place_cursor - 1 - self.places.folders.len())
                };
                self.focus = Focus::FileList;
                self.open_place(target)
            }
            _ => Vec::new(),
        }
    }

    fn handle_list_key(&mut self, key: Key, mods: KeyMods) -> Vec<PickerEffect> {
        if mods.ctrl && matches!(key, Key::Char('a') | Key::Char('A')) && self.is_multiple() {
            self.selected.clear();
            let dir_mode = self.is_directory_mode();
            for (vi, &ri) in self.visible.iter().enumerate() {
                let is_dir = self.raw_entries[ri].is_dir;
                if dir_mode == is_dir {
                    self.selected.insert(vi);
                }
            }
            return self.refresh_preview_effect();
        }

        match key {
            Key::Backspace => self.go_up(),
            Key::Up => self.move_list_cursor(-1, mods.shift),
            Key::Down => self.move_list_cursor(1, mods.shift),
            Key::PageUp => self.move_list_cursor(-10, mods.shift),
            Key::PageDown => self.move_list_cursor(10, mods.shift),
            Key::Home => self.jump_list_cursor(0, mods.shift),
            Key::End => {
                if let Some(last) = self.visible.len().checked_sub(1) {
                    self.jump_list_cursor(last, mods.shift)
                } else {
                    Vec::new()
                }
            }
            Key::Space => {
                if let Some(c) = self.cursor {
                    if self.is_multiple() {
                        if !self.selected.remove(&c) {
                            self.selected.insert(c);
                        }
                    } else {
                        self.selected.clear();
                        self.selected.insert(c);
                    }
                    return self.refresh_preview_effect();
                }
                Vec::new()
            }
            Key::Enter => {
                if self.overwrite.is_some() {
                    return Vec::new();
                }
                if let Some(c) = self.cursor {
                    if self.selected.len() <= 1 {
                        if let Some(e) = self.visible_entry(c).cloned() {
                            if e.is_dir {
                                return self.navigate_to(e.path);
                            }
                        }
                    }
                }
                self.try_accept()
            }
            _ => Vec::new(),
        }
    }

    fn move_list_cursor(&mut self, delta: isize, extend: bool) -> Vec<PickerEffect> {
        if self.visible.is_empty() {
            return Vec::new();
        }
        let next = match self.cursor {
            Some(cur) => (cur as isize + delta).clamp(0, self.visible.len() as isize - 1) as usize,
            None => 0,
        };
        self.jump_list_cursor(next, extend)
    }

    fn jump_list_cursor(&mut self, target: usize, extend: bool) -> Vec<PickerEffect> {
        if target >= self.visible.len() {
            return Vec::new();
        }
        if extend && self.is_multiple() {
            let anchor = self
                .selection_anchor
                .unwrap_or(self.cursor.unwrap_or(target));
            self.selection_anchor = Some(anchor);
            let (lo, hi) = if anchor <= target {
                (anchor, target)
            } else {
                (target, anchor)
            };
            self.selected.clear();
            for i in lo..=hi {
                self.selected.insert(i);
            }
        } else {
            self.selected.clear();
            self.selected.insert(target);
            self.selection_anchor = Some(target);
        }
        self.cursor = Some(target);
        if matches!(self.mode, PickerMode::SaveFile) {
            if let Some(e) = self.visible_entry(target) {
                if !e.is_dir {
                    self.save_input.set_text(e.display_name.clone());
                    self.overwrite = None;
                }
            }
        }
        self.refresh_preview_effect()
    }

    fn cycle_focus(&mut self, reverse: bool) {
        let mut ring = vec![Focus::Places, Focus::FileList];
        if matches!(self.mode, PickerMode::SaveFile) {
            ring.push(Focus::SaveName);
        }
        if !self.filters.is_empty() {
            ring.push(Focus::FilterBtn);
        }
        ring.push(Focus::CancelBtn);
        ring.push(Focus::AcceptBtn);

        let pos = ring.iter().position(|&f| f == self.focus).unwrap_or(0);
        let next = if reverse {
            if pos == 0 {
                ring.len() - 1
            } else {
                pos - 1
            }
        } else {
            (pos + 1) % ring.len()
        };
        self.focus = ring[next];
        if self.focus == Focus::SaveName {
            self.save_input.select_stem_before_dot();
        }
    }

    fn refresh_preview_effect(&mut self) -> Vec<PickerEffect> {
        if !self.preview_open {
            return Vec::new();
        }
        let entries = self.selected_entries();
        if entries.is_empty() {
            self.preview = PreviewContent::Empty;
            return vec![PickerEffect::ClearPreview];
        }
        if entries.len() > 1 {
            let total_size: u64 = entries.iter().map(|e| e.size).sum();
            self.preview = PreviewContent::Multiple {
                count: entries.len(),
                total_size,
            };
            return vec![PickerEffect::ClearPreview];
        }
        let name = entries[0].display_name.clone();
        let path = entries[0].path.clone();
        self.preview = PreviewContent::Loading { name };
        vec![PickerEffect::LoadPreview(path)]
    }

    fn selected_preview_effect(&self) -> Option<PickerEffect> {
        if !self.preview_open {
            return None;
        }
        let entries = self.selected_entries();
        if entries.len() == 1 {
            Some(PickerEffect::LoadPreview(entries[0].path.clone()))
        } else {
            None
        }
    }
}

fn edit_text_input(ti: &mut TextInput, key: Key, mods: KeyMods) -> Vec<PickerEffect> {
    if mods.ctrl && !mods.alt {
        match key {
            Key::Char('a') | Key::Char('A') => {
                ti.select_all();
                return Vec::new();
            }
            Key::Char('c') | Key::Char('C') => {
                if let Some(sel) = ti.selected_text() {
                    return vec![PickerEffect::SetClipboard(sel.to_string())];
                }
                return Vec::new();
            }
            Key::Char('x') | Key::Char('X') => {
                if let Some(sel) = ti.cut() {
                    return vec![PickerEffect::SetClipboard(sel)];
                }
                return Vec::new();
            }
            _ => {}
        }
    }
    match key {
        Key::Left => ti.move_left(mods.ctrl, mods.shift),
        Key::Right => ti.move_right(mods.ctrl, mods.shift),
        Key::Home => ti.move_home(mods.shift),
        Key::End => ti.move_end(mods.shift),
        Key::Backspace => {
            ti.backspace(mods.ctrl);
        }
        Key::Delete => {
            ti.delete(mods.ctrl);
        }
        Key::Space => {
            ti.insert(" ");
        }
        Key::Char(c) if !mods.ctrl && !mods.alt => {
            let mut buf = [0u8; 4];
            ti.insert(c.encode_utf8(&mut buf));
        }
        _ => {}
    }
    Vec::new()
}

fn is_dir_writable(dir: &Path) -> bool {
    let Ok(c_path) = CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK | libc::X_OK) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portal::protocol::{
        base64_encode, ChoiceOption, FilterPattern, PortalFilter, PortalOptions,
    };

    fn make_fixture() -> PathBuf {
        static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "infynitae-files-mode-test-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Docs")).unwrap();
        fs::create_dir_all(dir.join("Pictures")).unwrap();
        fs::write(dir.join("notes.md"), b"# Notes\n").unwrap();
        fs::write(dir.join("photo.png"), b"png").unwrap();
        fs::write(dir.join(".secret"), b"dot").unwrap();
        fs::write(dir.join("Docs/report.pdf"), b"pdf").unwrap();
        dir
    }

    fn base_req(method: &str, dir: &Path, options: PortalOptions) -> PortalRequest {
        let mut opts = options;
        let mut raw = dir.as_os_str().as_bytes().to_vec();
        raw.push(0);
        opts.current_folder_b64 = Some(base64_encode(&raw));
        PortalRequest {
            method: method.into(),
            app_id: "test.app".into(),
            parent_window: String::new(),
            title: String::new(),
            options: opts,
        }
    }

    #[test]
    fn open_file_filters_search_hidden_and_multiple_select() {
        let dir = make_fixture();
        let req = base_req(
            "OpenFile",
            &dir,
            PortalOptions {
                multiple: Some(true),
                filters: vec![PortalFilter {
                    label: "PNG".into(),
                    patterns: vec![FilterPattern {
                        kind: 0,
                        pattern: "*.png".into(),
                    }],
                }],
                ..Default::default()
            },
        );
        let (mut st, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            Vec::new(),
        );
        st.xbel_override = Some(dir.join("recent.xbel"));
        st.load_current_dir_sync();

        // Active filter is PNG: dirs Docs, Pictures + photo.png (3 items); .secret hidden.
        let names: Vec<String> = st
            .visible
            .iter()
            .map(|&i| st.raw_entries[i].display_name.clone())
            .collect();
        assert_eq!(names, vec!["Docs", "Pictures", "photo.png"]);

        // Switch to "All files" (index 1) and show hidden (Ctrl+H).
        st.select_filter(1);
        st.handle_key(
            Key::Char('h'),
            KeyMods {
                ctrl: true,
                ..Default::default()
            },
        );
        let all_names: Vec<String> = st
            .visible
            .iter()
            .map(|&i| st.raw_entries[i].display_name.clone())
            .collect();
        assert!(all_names.contains(&".secret".to_string()));
        assert!(all_names.contains(&"notes.md".to_string()));

        // Ctrl+A selects all files (not directories).
        st.handle_key(
            Key::Char('a'),
            KeyMods {
                ctrl: true,
                ..Default::default()
            },
        );
        let fx = st.try_accept();
        let Some(PickerEffect::Finish(resp)) = fx.last() else {
            panic!("expected Finish, got {fx:?}");
        };
        assert_eq!(resp.response, 0);
        assert_eq!(resp.writable, Some(true));
        assert_eq!(resp.uris.len(), 3); // .secret, notes.md, photo.png

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ctrl_l_path_typing_and_tab_completion_navigate() {
        let dir = make_fixture();
        let req = base_req("OpenFile", &dir, PortalOptions::default());
        let (mut st, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            Vec::new(),
        );
        st.load_current_dir_sync();

        st.handle_key(
            Key::Char('l'),
            KeyMods {
                ctrl: true,
                ..Default::default()
            },
        );
        assert!(st.path_editing);
        st.path_input.set_text("~/Do");
        st.handle_key(Key::Tab, KeyMods::default());
        assert_eq!(st.path_input.as_str(), "~/Docs/");

        let fx = st.handle_key(Key::Enter, KeyMods::default());
        assert!(!st.path_editing);
        assert_eq!(st.current_dir, dir.join("Docs"));
        assert!(fx.contains(&PickerEffect::LoadDir(dir.join("Docs"))));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_file_overwrite_check_and_choices() {
        let dir = make_fixture();
        let req = base_req(
            "SaveFile",
            &dir,
            PortalOptions {
                current_name: Some("notes.md".into()),
                choices: vec![
                    PortalChoice {
                        id: "enc".into(),
                        label: "Encoding".into(),
                        options: vec![
                            ChoiceOption {
                                id: "utf8".into(),
                                label: "UTF-8".into(),
                            },
                            ChoiceOption {
                                id: "latin1".into(),
                                label: "Latin-1".into(),
                            },
                        ],
                        initial: "utf8".into(),
                    },
                    PortalChoice {
                        id: "bom".into(),
                        label: "Add BOM".into(),
                        options: vec![],
                        initial: "false".into(),
                    },
                ],
                ..Default::default()
            },
        );
        let (mut st, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            Vec::new(),
        );
        st.xbel_override = Some(dir.join("recent.xbel"));
        st.load_current_dir_sync();

        // Stem before dot is selected initially.
        assert_eq!(st.save_input.selected_text(), Some("notes"));

        // Accepting existing file triggers inline overwrite prompt, not immediate finish.
        let fx = st.try_accept();
        assert!(fx.is_empty());
        assert!(matches!(st.overwrite, Some(OverwritePrompt::Single { .. })));

        // First Escape dismisses overwrite bar instead of cancelling window.
        let fx_esc = st.handle_key(Key::Escape, KeyMods::default());
        assert!(fx_esc.is_empty());
        assert!(st.overwrite.is_none());

        // Toggle boolean choice & confirm replace.
        st.toggle_bool_choice(1);
        st.try_accept();
        let fx_rep = st.confirm_overwrite_replace();
        let Some(PickerEffect::Finish(resp)) = fx_rep.last() else {
            panic!("expected Finish");
        };
        assert_eq!(resp.response, 0);
        assert_eq!(resp.uris, vec![path_to_file_uri(&dir.join("notes.md"))]);
        assert_eq!(
            resp.choices,
            Some(vec![
                ("enc".into(), "utf8".into()),
                ("bom".into(), "true".into()),
            ])
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_folder_and_directory_mode_and_escape_priority() {
        let dir = make_fixture();
        let req = base_req(
            "OpenFile",
            &dir,
            PortalOptions {
                directory: Some(true),
                ..Default::default()
            },
        );
        let (mut st, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            Vec::new(),
        );
        st.xbel_override = Some(dir.join("recent.xbel"));
        st.load_current_dir_sync();

        // Ctrl+Shift+N opens inline bar; typing and Enter creates the folder.
        st.handle_key(
            Key::Char('N'),
            KeyMods {
                ctrl: true,
                shift: true,
                alt: false,
            },
        );
        assert!(st.new_folder.is_some());
        for ch in "Archive".chars() {
            st.handle_key(Key::Char(ch), KeyMods::default());
        }
        st.handle_key(Key::Enter, KeyMods::default());
        assert!(dir.join("Archive").is_dir());
        st.load_current_dir_sync();

        // Ctrl+F search -> Escape clears search first, second Escape cancels dialog.
        st.handle_key(
            Key::Char('f'),
            KeyMods {
                ctrl: true,
                ..Default::default()
            },
        );
        st.handle_key(Key::Char('a'), KeyMods::default());
        assert_eq!(st.search_input.as_str(), "a");
        let fx1 = st.handle_key(Key::Escape, KeyMods::default());
        assert!(fx1.iter().all(|e| !matches!(e, PickerEffect::Finish(_))));
        assert!(st.search_input.is_empty());

        let fx2 = st.handle_key(Key::Escape, KeyMods::default());
        assert_eq!(fx2, vec![PickerEffect::Finish(PortalResponse::cancelled())]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_files_collision_prompt_renames_colliding() {
        let dir = make_fixture();
        let req = base_req(
            "SaveFiles",
            &dir,
            PortalOptions {
                files_b64: vec![base64_encode(b"notes.md\0"), base64_encode(b"fresh.txt\0")],
                ..Default::default()
            },
        );
        let (mut st, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            Vec::new(),
        );
        st.xbel_override = Some(dir.join("recent.xbel"));
        st.load_current_dir_sync();

        let fx = st.try_accept();
        assert!(fx.is_empty());
        assert!(matches!(st.overwrite, Some(OverwritePrompt::Multi { .. })));

        let fx2 = st.confirm_overwrite_rename_colliding();
        let Some(PickerEffect::Finish(resp)) = fx2.last() else {
            panic!("expected Finish");
        };
        assert_eq!(
            resp.uris,
            vec![
                path_to_file_uri(&dir.join("notes (2).md")),
                path_to_file_uri(&dir.join("fresh.txt")),
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_view_history_and_current_file_precedence() {
        let dir = make_fixture();
        let report = dir.join("Docs/report.pdf");
        let recent = vec![RecentItem {
            path: report.clone(),
            modified_secs: Some(1_700_000_000),
        }];
        let req = base_req("OpenFile", &dir, PortalOptions::default());
        let (mut st, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            recent,
        );
        st.xbel_override = Some(dir.join("recent.xbel"));
        st.load_current_dir_sync();

        // Switch to Recent view, pick the recent file, and test Back/Forward history.
        st.open_recent_view();
        assert!(st.in_recent_view);
        assert_eq!(st.visible.len(), 1);
        assert_eq!(
            st.visible_entry(0).unwrap().subtitle.as_deref(),
            Some("~/Docs")
        );
        let fx = st.try_accept();
        let Some(PickerEffect::Finish(resp)) = fx.last() else {
            panic!("expected Finish from Recent");
        };
        assert_eq!(resp.uris, vec![path_to_file_uri(&report)]);

        st.go_back();
        assert!(!st.in_recent_view);
        assert_eq!(st.current_dir, dir);
        assert!(st.can_go_forward());
        st.go_forward();
        assert!(st.in_recent_view);

        // SaveFile current_file overrides both current_folder and current_name.
        let mut cf_raw = report.as_os_str().as_bytes().to_vec();
        cf_raw.push(0);
        let save_req = base_req(
            "SaveFile",
            &dir,
            PortalOptions {
                current_name: Some("ignored.txt".into()),
                current_file_b64: Some(base64_encode(&cf_raw)),
                ..Default::default()
            },
        );
        let (st_save, _) = PickerState::from_request(
            save_req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            Vec::new(),
        );
        assert_eq!(st_save.current_dir, dir.join("Docs"));
        assert_eq!(st_save.save_input.as_str(), "report.pdf");
        assert_eq!(st_save.save_input.selected_text(), Some("report"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_file_rejects_unwritable_directory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = make_fixture();
        let ro = dir.join("readonly");
        fs::create_dir_all(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();

        let req = base_req(
            "SaveFile",
            &ro,
            PortalOptions {
                current_name: Some("out.txt".into()),
                ..Default::default()
            },
        );
        let (mut st, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            Some(dir.clone()),
            Vec::new(),
        );
        st.load_current_dir_sync();

        let fx = st.try_accept();
        // Root bypasses DAC write checks; skip assertion only if running as uid 0.
        if unsafe { libc::geteuid() } != 0 {
            assert!(fx.is_empty());
            assert!(st
                .error_msg
                .as_deref()
                .is_some_and(|m| m.contains("not writable")));
        }

        let _ = fs::set_permissions(&ro, fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&dir);
    }
}
