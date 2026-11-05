//! iPhone and iPad have no menu bar: the same API as `menu.rs`, doing nothing.
use crate::keys::{Action, Binding};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, PartialEq, Default)]
pub struct Shape {
    pub keymap: Vec<(Action, Binding)>,
    pub perspectives: Vec<(String, String)>,
    pub recent: Vec<PathBuf>,
    pub git: bool,
    pub sidebar: bool,
    pub inspector: bool,
    pub board: bool,
}

pub struct Bar {
    pub shape: Shape,
}

pub fn listen(_ctx: &egui::Context) -> Receiver<String> {
    channel().1
}

pub fn build(s: &Shape) -> Bar {
    Bar { shape: s.clone() }
}

#[allow(dead_code)] // only the Mac's menu bar produces these
#[derive(Debug, PartialEq)]
pub enum Cmd {
    Event(egui::Event),
    Run(Action),
    Other(String),
}

pub fn decode(id: &str, _keymap: &[(Action, Binding)]) -> Cmd {
    Cmd::Other(id.to_string())
}
