#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialogue;

use crate::app::run;

fn main() {
    run().unwrap_or_else(|e| panic!("{}", e));
}