//! In-app file/folder browser for this machine. Native dialogs (`rfd`) can fail
//! silently without an xdg portal on Wayland, so every "choose a file/folder"
//! flow uses this sheet. One instance lives in `Shared`; a flow opens it with a
//! tag and later collects the result with `Shared::take_picked(tag)`.
use crate::panels::workkit;
use crate::theme::{self, ty, Palette, W};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, Context};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum PickMode {
    Folder,
    /// Files whose extension is in the list (empty = any file).
    File(&'static [&'static str]),
    /// Several files at once (click toggles; empty list = any file).
    Files(&'static [&'static str]),
    /// Choose a folder and type a file name; the result is `dir/name`.
    Save,
}

#[derive(Default)]
pub struct LocalPicker {
    open: bool,
    tag: String,
    title: String,
    mode: Option<PickMode>,
    dir: PathBuf,
    path_text: String,
    entries: Vec<(String, bool)>,
    sel: Option<String>,
    multi: Vec<String>,
    save_name: String,
    hidden: bool,
    error: Option<String>,
}

pub fn list_dir(dir: &Path, mode: PickMode, hidden: bool) -> Result<Vec<(String, bool)>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut v: Vec<(String, bool)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                return None;
            }
            let is_dir = e.path().is_dir();
            match mode {
                PickMode::Folder if !is_dir => return None,
                PickMode::File(exts) | PickMode::Files(exts) if !is_dir && !exts.is_empty() => {
                    let ext = Path::new(&name).extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
                    if !exts.contains(&ext.as_str()) {
                        return None;
                    }
                }
                _ => {}
            }
            Some((name, is_dir))
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    Ok(v)
}

impl LocalPicker {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, tag: &str, title: &str, mode: PickMode, start: Option<PathBuf>) {
        self.open_named(tag, title, mode, start, "");
    }

    /// Opens a Save sheet pre-filled with `name`.
    pub fn open_save(&mut self, tag: &str, title: &str, name: &str, start: Option<PathBuf>) {
        self.open_named(tag, title, PickMode::Save, start.or_else(dirs::download_dir), name);
    }

    fn open_named(&mut self, tag: &str, title: &str, mode: PickMode, start: Option<PathBuf>, name: &str) {
        self.multi.clear();
        self.save_name = name.into();
        self.open = true;
        self.tag = tag.into();
        self.title = title.into();
        self.mode = Some(mode);
        self.sel = None;
        let dir = start.filter(|d| d.is_dir()).or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("/"));
        self.go(dir);
    }

    fn go(&mut self, dir: PathBuf) {
        let mode = self.mode.unwrap_or(PickMode::Folder);
        match list_dir(&dir, mode, self.hidden) {
            Ok(e) => {
                self.entries = e;
                self.error = None;
                self.path_text = dir.to_string_lossy().into_owned();
                self.dir = dir;
                self.sel = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Draws the sheet; returns `(tag, path)` pairs when the user chose something.
    pub fn ui(&mut self, ctx: &Context, p: &Palette) -> Vec<(String, PathBuf)> {
        if !self.open {
            return Vec::new();
        }
        let mode = self.mode.unwrap_or(PickMode::Folder);
        let mut result: Option<PathBuf> = None;
        let cancel = false;
        let mut cancel2 = false;
        let mut nav: Option<PathBuf> = None;
        let title = self.title.clone();
        let choose_path = match &self.sel {
            Some(s) => self.dir.join(s),
            None => self.dir.clone(),
        };
        let mut chosen: Option<PathBuf> = None;
        let mut multi_done = false;
        let save_path = self.dir.join(crate::panels::attachments::safe_name(self.save_name.trim()));
        let can_choose = match mode {
            PickMode::Folder => true,
            PickMode::File(_) => self.sel.is_some(),
            PickMode::Files(_) => !self.multi.is_empty(),
            PickMode::Save => !self.save_name.trim().is_empty(),
        };
        let go_label = match mode {
            PickMode::Folder => "Choose",
            PickMode::Save => "Save",
            _ => "Open",
        };
        let esc = workkit::sheet(
            ctx,
            p,
            "local_picker",
            &title,
            520.0,
            |ui| {
                if widgets::button_ex(ui, p, BtnKind::Primary, true, can_choose, go_label).clicked() {
                    match mode {
                        PickMode::Files(_) => multi_done = true,
                        PickMode::Save => chosen = Some(save_path.clone()),
                        _ => chosen = Some(choose_path.clone()),
                    }
                }
                if widgets::button_ex(ui, p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                    cancel2 = true;
                }
            },
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let home = dirs::home_dir();
                    let mut places: Vec<(&str, Option<PathBuf>)> = vec![("Home", home.clone())];
                    for (l, f) in [("Desktop", "Desktop"), ("Documents", "Documents"), ("Downloads", "Downloads"), ("Pictures", "Pictures"), ("Videos", "Videos")] {
                        places.push((l, home.as_ref().map(|h| h.join(f))));
                    }
                    for (l, d) in places {
                        if let Some(d) = d.filter(|d| d.is_dir()) {
                            if widgets::button_ex(ui, p, BtnKind::Secondary, true, true, l).clicked() {
                                nav = Some(d);
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if widgets::icon_btn(ui, p, "chevron.up", 26.0, false).on_hover_text("Parent folder").clicked() {
                        if let Some(par) = self.dir.parent() {
                            nav = Some(par.to_path_buf());
                        }
                    }
                    let r = widgets::field(ui, p, &mut self.path_text, "Path");
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let t = PathBuf::from(self.path_text.trim());
                        if t.is_dir() {
                            nav = Some(t);
                        } else {
                            self.error = Some("Not a folder".into());
                        }
                    }
                });
                if let Some(e) = &self.error {
                    ui.label(ty::sized(e, 11.0, W::Regular).color(p.danger));
                }
                egui::ScrollArea::vertical().id_salt("local_picker_list").max_height(280.0).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    if self.entries.is_empty() {
                        ui.label(ty::caption(if mode == PickMode::Folder { "No sub-folders" } else { "Nothing here" }).color(p.ink3));
                    }
                    for (name, is_dir) in self.entries.clone() {
                        let selected = self.sel.as_deref() == Some(name.as_str()) || self.multi.contains(&name);
                        let r = widgets::hover_row(ui, p, selected, theme::R_CTRL, |ui| {
                            ui.horizontal(|ui| {
                                crate::icons::symbol(ui, if is_dir { "folder" } else { "doc" }, 13.0, if is_dir { p.accent } else { p.ink3 });
                                ui.label(ty::sized(&name, 12.5, W::Regular).color(p.ink));
                            });
                        });
                        if r.double_clicked() {
                            if is_dir {
                                nav = Some(self.dir.join(&name));
                            } else if mode == PickMode::Save {
                                self.save_name = name.clone();
                            } else if !matches!(mode, PickMode::Files(_)) {
                                result = Some(self.dir.join(&name));
                            }
                        } else if r.clicked() {
                            if matches!(mode, PickMode::Files(_)) && !is_dir {
                                if let Some(i) = self.multi.iter().position(|n| *n == name) {
                                    self.multi.remove(i);
                                } else {
                                    self.multi.push(name.clone());
                                }
                            } else if mode == PickMode::Save && !is_dir {
                                self.save_name = name.clone();
                            } else {
                                self.sel = Some(name.clone());
                            }
                        }
                    }
                });
                if mode == PickMode::Save {
                    ui.horizontal(|ui| {
                        ui.label(ty::sized("Name", 11.5, W::Medium).color(p.ink3));
                        widgets::field(ui, p, &mut self.save_name, "File name");
                    });
                }
                if ui.checkbox(&mut self.hidden, "Show hidden").changed() {
                    nav = Some(self.dir.clone());
                }
            },
        );
        if let Some(d) = nav {
            self.go(d);
        }
        if cancel || cancel2 || esc {
            self.open = false;
            return Vec::new();
        }
        if multi_done {
            self.open = false;
            return std::mem::take(&mut self.multi).into_iter().map(|n| (self.tag.clone(), self.dir.join(n))).collect();
        }
        if let Some(path) = result.or(chosen) {
            self.open = false;
            return vec![(self.tag.clone(), path)];
        }
        Vec::new()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_folders_first_and_filters_files() {
        let d = std::env::temp_dir().join(format!("jb-pick-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.png"), b"x").unwrap();
        std::fs::write(d.join("b.txt"), b"x").unwrap();
        let v = list_dir(&d, PickMode::File(&["png"]), false).unwrap();
        assert_eq!(v, vec![("sub".to_string(), true), ("a.png".to_string(), false)]);
        assert_eq!(list_dir(&d, PickMode::Folder, false).unwrap().len(), 1);
        std::fs::remove_dir_all(&d).ok();
    }
}
