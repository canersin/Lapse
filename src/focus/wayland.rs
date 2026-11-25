//! The focused window as the Wayland compositor lists it: wlr-foreign-toplevel (Sway,
//! Hyprland, niri, labwc, Wayfire, river, mango, ...) or COSMIC's own protocol.

use super::{Report, Source, Window};
use std::collections::HashMap;
use std::time::Duration;
use wayland_client::backend::ObjectId;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

mod cosmic {
    pub mod client {
        use wayland_client;
        use wayland_client::protocol::*;
        use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1;

        pub mod __interfaces {
            use wayland_client::protocol::__interfaces::*;
            use wayland_protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
            wayland_scanner::generate_interfaces!("src/focus/protocols/cosmic-toplevel-info.xml");
        }
        use self::__interfaces::*;
        wayland_scanner::generate_client_code!("src/focus/protocols/cosmic-toplevel-info.xml");
    }
}
use cosmic::client::zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1};
use cosmic::client::zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1};

/// `activated` and `fullscreen` in both protocols' state lists.
const ACTIVATED: u32 = 2;
const FULLSCREEN: u32 = 3;

#[derive(Default)]
struct Toplevel {
    class: String,
    title: String,
    activated: bool,
    fullscreen: bool,
    output: Option<ObjectId>,
    /// COSMIC: the handle with the state, by which this window is found again.
    cosmic: Option<ObjectId>,
}

struct Output {
    global: u32,
    name: Option<String>,
}

#[derive(Default)]
struct State {
    toplevels: HashMap<ObjectId, Toplevel>,
    outputs: HashMap<ObjectId, Output>,
    cosmic: Option<ZcosmicToplevelInfoV1>,
    changed: bool,
}

impl State {
    fn by_cosmic(&mut self, handle: &ZcosmicToplevelHandleV1) -> Option<&mut Toplevel> {
        let id = handle.id();
        self.toplevels.values_mut().find(|t| t.cosmic.as_ref() == Some(&id))
    }

    fn focused(&self) -> Report {
        match self.toplevels.values().find(|t| t.activated) {
            Some(t) => Report::Window(Window {
                pid: None,
                class: t.class.clone(),
                title: t.title.clone(),
                monitor: t.output.as_ref().and_then(|o| self.outputs.get(o)).and_then(|o| o.name.clone()),
                x11: None,
            }),
            None => Report::Nothing,
        }
    }

    fn toplevels_list(&self) -> Vec<super::clients::Toplevel> {
        let mut list: Vec<_> = self.toplevels.values().map(|t| super::clients::Toplevel {
            class: t.class.clone(),
            fullscreen: t.fullscreen,
        }).collect();
        list.sort_by(|a, b| a.class.cmp(&b.class).then(a.fullscreen.cmp(&b.fullscreen)));
        list
    }
}

fn activated(state: &[u8]) -> bool {
    state.chunks_exact(4).any(|c| c == ACTIVATED.to_ne_bytes())
}

fn fullscreen(state: &[u8]) -> bool {
    state.chunks_exact(4).any(|c| c == FULLSCREEN.to_ne_bytes())
}

pub fn run() {
    loop {
        let res = watch();
        super::set_toplevels(Vec::new());
        super::set_monitor_count(Source::Compositor, 0);
        super::report(Source::Compositor, None);
        match res {
            // This compositor doesn't tell.
            Ok(()) => return,
            Err(e) => eprintln!("Lapse: Wayland focus watcher: {e}"),
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn watch() -> anyhow::Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none_or(|d| d.is_empty()) {
        return Ok(());
    }
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let mut state = State::default();

    let _manager = globals.bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ()).ok();
    let mut _list = None;
    if _manager.is_none() {
        state.cosmic = globals.bind::<ZcosmicToplevelInfoV1, _, _>(&qh, 2..=2, ()).ok();
        if state.cosmic.is_none() {
            return Ok(());
        }
        _list = Some(globals.bind::<ExtForeignToplevelListV1, _, _>(&qh, 1..=1, ())?);
    }
    globals.contents().with_list(|list| {
        for g in list.iter().filter(|g| g.interface == wl_output::WlOutput::interface().name && g.version >= 4) {
            let output = globals.registry().bind::<wl_output::WlOutput, _, _>(g.name, 4, &qh, ());
            state.outputs.insert(output.id(), Output { global: g.name, name: None });
        }
    });
    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;

    let mut last = None;
    let mut last_outputs = None;
    let mut last_toplevels: Option<Vec<super::clients::Toplevel>> = None;
    loop {
        let count = state.outputs.len();
        if last_outputs != Some(count) {
            super::set_monitor_count(Source::Compositor, count);
            last_outputs = Some(count);
        }
        let toplevels = state.toplevels_list();
        if last_toplevels.as_ref() != Some(&toplevels) {
            super::set_toplevels(toplevels.clone());
            last_toplevels = Some(toplevels);
        }
        let found = state.focused();
        if last.as_ref() != Some(&found) {
            super::report(Source::Compositor, Some(found.clone()));
            last = Some(found);
        }
        state.changed = false;
        while !state.changed {
            queue.blocking_dispatch(&mut state)?;
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &GlobalListContents, _: &Connection, qh: &QueueHandle<Self>) {
        match event {
            wl_registry::Event::Global { name, interface, version } => {
                if interface == wl_output::WlOutput::interface().name && version >= 4 {
                    let output = registry.bind::<wl_output::WlOutput, _, _>(name, 4, qh, ());
                    state.outputs.insert(output.id(), Output { global: name, name: None });
                    state.changed = true;
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                let len = state.outputs.len();
                state.outputs.retain(|_, o| o.global != name);
                if state.outputs.len() != len {
                    state.changed = true;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(state: &mut Self, output: &wl_output::WlOutput, event: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_output::Event::Name { name } = event {
            if let Some(o) = state.outputs.get_mut(&output.id()) {
                o.name = Some(name);
                state.changed = true;
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(state: &mut Self, _: &ZwlrForeignToplevelManagerV1, event: zwlr_foreign_toplevel_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.insert(toplevel.id(), Toplevel::default());
        }
    }

    event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(state: &mut Self, handle: &ZwlrForeignToplevelHandleV1, event: zwlr_foreign_toplevel_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        if let Event::Closed = event {
            state.toplevels.remove(&handle.id());
            handle.destroy();
            state.changed = true;
            return;
        }
        let Some(t) = state.toplevels.get_mut(&handle.id()) else { return };
        match event {
            Event::Title { title } => t.title = title,
            Event::AppId { app_id } => t.class = app_id,
            Event::OutputEnter { output } => t.output = Some(output.id()),
            Event::OutputLeave { output } if t.output == Some(output.id()) => t.output = None,
            Event::State { state: list } => {
                t.activated = activated(&list);
                t.fullscreen = fullscreen(&list);
            }
            // A whole set of changes is in. While the focus moves, one window may have lost
            // it before the next has it, so the new one is waited for (the events come together).
            Event::Done => state.changed = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for State {
    fn event(state: &mut Self, _: &ExtForeignToplevelListV1, event: ext_foreign_toplevel_list_v1::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            let cosmic = state.cosmic.as_ref().map(|info| info.get_cosmic_toplevel(&toplevel, qh, ()).id());
            state.toplevels.insert(toplevel.id(), Toplevel { cosmic, ..Default::default() });
        }
    }

    event_created_child!(State, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for State {
    fn event(state: &mut Self, handle: &ExtForeignToplevelHandleV1, event: ext_foreign_toplevel_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use ext_foreign_toplevel_handle_v1::Event;
        if let Event::Closed = event {
            state.toplevels.remove(&handle.id());
            handle.destroy();
            state.changed = true;
            return;
        }
        let Some(t) = state.toplevels.get_mut(&handle.id()) else { return };
        match event {
            Event::Title { title } => t.title = title,
            Event::AppId { app_id } => t.class = app_id,
            Event::Done => state.changed = true,
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for State {
    fn event(state: &mut Self, _: &ZcosmicToplevelInfoV1, event: zcosmic_toplevel_info_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zcosmic_toplevel_info_v1::Event::Done = event {
            state.changed = true;
        }
    }

    event_created_child!(State, ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (ZcosmicToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZcosmicToplevelHandleV1, ()> for State {
    fn event(state: &mut Self, handle: &ZcosmicToplevelHandleV1, event: zcosmic_toplevel_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use zcosmic_toplevel_handle_v1::Event;
        let Some(t) = state.by_cosmic(handle) else { return };
        match event {
            Event::OutputEnter { output } => t.output = Some(output.id()),
            Event::OutputLeave { output } if t.output == Some(output.id()) => t.output = None,
            Event::State { state: list } => {
                t.activated = activated(&list);
                t.fullscreen = fullscreen(&list);
            }
            _ => return,
        }
        state.changed = true;
    }
}
