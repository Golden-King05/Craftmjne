//! The resource-pack list, shared by Settings > Packs (main menu) and the pause menu (in a world).
//!
//! Two lists, two scopes:
//! - **Global** packs (`packs::PackSelection`, saved per launcher instance) apply to every world
//!   and to the menus.
//! - **This world's** packs (`WorldMeta::world_packs`, saved with the world) apply only inside it,
//!   stacked above the global ones. Only offered while a world is open.
//!
//! Either way a change applies at once: the menu skin rebuilds (`gui_skin`), and in a world the
//! content is rebuilt and every chunk remeshed (`world::refresh_content_live`).

use bevy::prelude::*;

use crate::packs::{InstalledPacks, PackSelection};
use crate::save::SaveStore;
use crate::state::{ActiveWorld, AppState};
use crate::ui_theme::{self as theme, ButtonOpts, ButtonStyle};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PackScope {
    #[default]
    Global,
    World,
}

/// Which list the panel is editing.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackPanel {
    pub scope: PackScope,
}

#[derive(Component, Clone, PartialEq, Debug)]
pub enum PackButton {
    Scope(PackScope),
    Toggle(String),
    Shift(String, bool),
    Rescan,
}

/// The list a scope edits, from the two places it lives.
pub fn list_for<'a>(scope: PackScope, global: &'a PackSelection, world: &'a [String]) -> &'a [String] {
    match scope {
        PackScope::Global => &global.enabled,
        PackScope::World => world,
    }
}

/// Turns `folder` on (top of the list) or off in `list`.
pub fn toggle(list: &mut Vec<String>, folder: &str) {
    if list.iter().any(|f| f == folder) {
        list.retain(|f| f != folder);
    } else {
        list.insert(0, folder.to_string());
    }
}

/// Moves `folder` one place up or down in `list`.
pub fn shift(list: &mut [String], folder: &str, up: bool) {
    let Some(i) = list.iter().position(|f| f == folder) else { return };
    let j = if up { i.checked_sub(1) } else { (i + 1 < list.len()).then_some(i + 1) };
    if let Some(j) = j {
        list.swap(i, j);
    }
}

fn btn(row: &mut ChildSpawnerCommands, label: &str, action: PackButton, style: ButtonStyle, w: f32, selected: bool) {
    theme::button(row, action, label, ButtonOpts::new(style).size(w, 34.0).font(15.0).selected(selected).text(theme::TEXT));
}

/// Spawns the panel's contents: scope tabs (when `in_world`), the list, and rescan.
pub fn spawn_pack_panel(body: &mut ChildSpawnerCommands, scope: PackScope, in_world: bool, global: &PackSelection, world: &[String], installed: &InstalledPacks) {
    let scope = if in_world { scope } else { PackScope::Global };
    body.spawn((Text::new("Resource packs"), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
    if in_world {
        body.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|row| {
            btn(row, "Global (every world)", PackButton::Scope(PackScope::Global), ButtonStyle::Choice, 220.0, scope == PackScope::Global);
            btn(row, "This world", PackButton::Scope(PackScope::World), ButtonStyle::Choice, 160.0, scope == PackScope::World);
        });
    }
    let list = list_for(scope, global, world);
    for folder in list {
        let info = installed.0.iter().find(|p| &p.folder == folder);
        body.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|row| {
            btn(row, "Up", PackButton::Shift(folder.clone(), true), ButtonStyle::Normal, 56.0, false);
            btn(row, "Down", PackButton::Shift(folder.clone(), false), ButtonStyle::Normal, 64.0, false);
            btn(row, "Off", PackButton::Toggle(folder.clone()), ButtonStyle::Choice, 56.0, true);
            let (label, colour) = match info {
                Some(p) => (p.name.clone(), theme::TEXT),
                None => (format!("{folder} (missing)"), theme::ERROR),
            };
            row.spawn((Text::new(label), TextFont { font_size: 16.0, ..default() }, TextColor(colour)));
        });
    }
    for p in installed.0.iter().filter(|p| !list.contains(&p.folder)) {
        body.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|row| {
            btn(row, "On", PackButton::Toggle(p.folder.clone()), ButtonStyle::Choice, 56.0, false);
            row.spawn((Text::new(p.name.clone()), TextFont { font_size: 16.0, ..default() }, TextColor(theme::TEXT_DIM)));
        });
    }
    if installed.0.is_empty() && list.is_empty() {
        theme::hint(body, "No resource packs installed. Put a pack folder in the resourcepacks folder next to the game.", 14.0);
    }
    btn(body, "Rescan", PackButton::Rescan, ButtonStyle::Normal, 120.0, false);
    let note = match scope {
        PackScope::Global => "Global packs apply to every world and the menus, right away. Packs higher in the list win.",
        PackScope::World => "Packs for this world only: they sit above your global packs here, and apply right away.",
    };
    theme::hint(body, format!("{note}\nA mod's own pack always loads with the mod and can't be switched here."), 13.0);
}

/// Applies a click: tab, toggle, reorder or rescan, saving whichever list changed.
#[allow(clippy::too_many_arguments)]
fn handle_pack_buttons(
    buttons: Query<(&Interaction, &PackButton), Changed<Interaction>>,
    mut panel: ResMut<PackPanel>,
    mut selection: ResMut<PackSelection>,
    mut installed: ResMut<InstalledPacks>,
    mut world: Option<ResMut<ActiveWorld>>,
    state: Option<Res<State<AppState>>>,
    store: Res<SaveStore>,
) {
    let in_world = state.is_some_and(|s| *s.get() == AppState::InGame) && world.is_some();
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let scope = if in_world { panel.scope } else { PackScope::Global };
        let edit = |f: &mut dyn FnMut(&mut Vec<String>), selection: &mut PackSelection, world: &mut Option<ResMut<ActiveWorld>>| match (scope, world.as_mut()) {
            (PackScope::World, Some(w)) => {
                f(&mut w.meta.world_packs);
                let slug = w.slug.clone();
                let _ = store.save_meta(&slug, &w.meta);
            }
            _ => {
                f(&mut selection.enabled);
                let _ = store.save_pack_selection(selection);
            }
        };
        match button.clone() {
            PackButton::Scope(s) => panel.scope = s,
            PackButton::Toggle(f) => edit(&mut |l| toggle(l, &f), &mut selection, &mut world),
            PackButton::Shift(f, up) => edit(&mut |l| shift(l, &f, up), &mut selection, &mut world),
            PackButton::Rescan => installed.0 = crate::packs::scan_packs(&crate::packs::user_packs_dir()),
        }
    }
}

pub struct PackPanelPlugin;

impl Plugin for PackPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PackPanel>().add_systems(Update, handle_pack_buttons);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn toggling_adds_on_top_and_removes() {
        let mut l = v(&["a"]);
        toggle(&mut l, "b");
        assert_eq!(l, ["b", "a"]);
        toggle(&mut l, "a");
        assert_eq!(l, ["b"]);
    }

    #[test]
    fn shifting_stops_at_the_ends() {
        let mut l = v(&["a", "b", "c"]);
        shift(&mut l, "a", true);
        assert_eq!(l, ["a", "b", "c"]);
        shift(&mut l, "c", true);
        assert_eq!(l, ["a", "c", "b"]);
        shift(&mut l, "b", false);
        assert_eq!(l, ["a", "c", "b"]);
    }

    #[test]
    fn each_scope_reads_its_own_list() {
        let global = PackSelection { enabled: v(&["g"]) };
        let world = v(&["w"]);
        assert_eq!(list_for(PackScope::Global, &global, &world), ["g"]);
        assert_eq!(list_for(PackScope::World, &global, &world), ["w"]);
    }
}
