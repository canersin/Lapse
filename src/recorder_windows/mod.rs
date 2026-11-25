#[cfg_attr(not(windows), allow(dead_code))]
pub mod segment;

#[cfg_attr(not(windows), allow(dead_code))]
pub mod aac_track;

#[cfg_attr(not(windows), allow(dead_code))]
pub mod audio;

#[cfg_attr(not(windows), allow(dead_code))]
pub mod mem_store;

#[cfg_attr(not(windows), allow(dead_code))]
pub mod mp4_box;

pub mod focus_card;

pub mod focus_gate;

#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod remux;

#[cfg(windows)]
use libc as _;

