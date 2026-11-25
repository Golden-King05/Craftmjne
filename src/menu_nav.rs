//! Keyboard navigation for the point-and-click menus: arrow keys (or Tab / Shift+Tab)
//! move a highlight between buttons, Enter presses the highlighted one.
//!
//! The highlight is [`MenuFocus`]; `ui_theme` draws a focused button with its hover
//! texture, so it looks exactly like the mouse is on it. Pressing is done by setting the
//! button's `Interaction` to `Pressed` for one frame, which is what every screen's own
//! click handler already reacts to - no screen needed any change. A controller does the
//! same: D-pad or a flick of the left stick moves, A (south) presses. Moving the mouse onto a
//! button drops the keyboard highlight so the two never disagree.
//!
//! It runs only where it can't fight something else for the keys: the main menu, the mods
//! list, the worlds list and world options, and the pause menu. The create-world form
//! (typing) and Settings (typing into search, capturing a key to bind) are left alone.
//! The keys here are fixed, not rebindable controls: menus have to work before anyone has
//! touched the controls screen.

use bevy::prelude::*;
use bevy::ui::UiSystem;

use crate::gamepad::{PadButton, PadState};
use crate::menu::WorldsScreenMode;
use crate::inventory::InventoryState;
use crate::knap_ui::KnapState;
use crate::state::{AppState, DeathState, PauseState};

/// Which button the keyboard has highlighted.
#[derive(Resource, Default, Debug)]
pub struct MenuFocus {
    pub entity: Option<Entity>,
    /// A button this system pressed last frame; released at the start of the next.
    pressed: Option<Entity>,
    /// The direction the left stick was held in last frame, so a held stick moves once.
    stick: Option<Dir>,
    /// Where the highlight was, kept for when a rebuild replaces its button.
    last_center: Option<Vec2>,
    /// Frames to wait for a rebuilt screen to be laid out before re-highlighting.
    restore_in: u8,
    /// The press this frame was the secondary action (X): shift-click in the inventory.
    /// Read by the screens' click handlers instead of Shift.
    pub secondary: bool,
}

/// How long to wait after a screen is rebuilt: the new nodes need a frame to get positions.
const RESTORE_FRAMES: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
    /// Reading order forwards / backwards (Tab, Shift+Tab).
    Next,
    Prev,
}

/// Where a button is: centre and size, in the same units for every button.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub center: Vec2,
    pub size: Vec2,
}

/// Which of `spots` a key press moves to from `current` (or the first in reading order
/// when nothing is highlighted yet). Pure, so it is tested without an app.
pub fn navigate(spots: &[Spot], current: Option<usize>, dir: Dir) -> Option<usize> {
    if spots.is_empty() {
        return None;
    }
    // Reading order: top to bottom, then left to right (rows are bands of half a button).
    let mut order: Vec<usize> = (0..spots.len()).collect();
    order.sort_by(|&a, &b| {
        let (sa, sb) = (spots[a], spots[b]);
        let same_row = (sa.center.y - sb.center.y).abs() < sa.size.y.min(sb.size.y) * 0.5;
        if same_row {
            sa.center.x.total_cmp(&sb.center.x)
        } else {
            sa.center.y.total_cmp(&sb.center.y)
        }
    });
    let Some(cur) = current.filter(|c| *c < spots.len()) else {
        return Some(match dir {
            Dir::Up | Dir::Left | Dir::Prev => *order.last().unwrap(),
            _ => order[0],
        });
    };
    let pos = order.iter().position(|&i| i == cur).unwrap();
    let wrap = |p: isize| order[p.rem_euclid(order.len() as isize) as usize];
    let c = spots[cur];
    let half = c.size.y * 0.5;
    // The nearest button strictly in a direction; `None` when there isn't one.
    let nearest = |keep: &dyn Fn(Vec2) -> bool, cost: &dyn Fn(Vec2) -> f32| {
        (0..spots.len())
            .filter(|&i| i != cur && keep(spots[i].center - c.center))
            .min_by(|&a, &b| cost(spots[a].center - c.center).total_cmp(&cost(spots[b].center - c.center)))
    };
    match dir {
        Dir::Next => Some(wrap(pos as isize + 1)),
        Dir::Prev => Some(wrap(pos as isize - 1)),
        Dir::Down => nearest(&|d| d.y > half, &|d| d.y * 4.0 + d.x.abs()).or_else(|| Some(wrap(0))),
        Dir::Up => nearest(&|d| d.y < -half, &|d| -d.y * 4.0 + d.x.abs()).or_else(|| Some(wrap(-1))),
        Dir::Right => nearest(&|d| d.y.abs() <= half && d.x > 0.0, &|d| d.x).or_else(|| Some(wrap(pos as isize + 1))),
        Dir::Left => nearest(&|d| d.y.abs() <= half && d.x < 0.0, &|d| -d.x).or_else(|| Some(wrap(pos as isize - 1))),
    }
}

/// The index of the point closest to `at`.
pub fn nearest_to(points: &[Vec2], at: Vec2) -> Option<usize> {
    (0..points.len()).min_by(|&a, &b| points[a].distance_squared(at).total_cmp(&points[b].distance_squared(at)))
}

/// A direction from a fresh D-pad press.
fn pad_direction(pad: &PadState) -> Option<Dir> {
    for (b, d) in [(PadButton::DPadUp, Dir::Up), (PadButton::DPadDown, Dir::Down), (PadButton::DPadLeft, Dir::Left), (PadButton::DPadRight, Dir::Right)] {
        if pad.started(b) {
            return Some(d);
        }
    }
    None
}

/// The dominant direction of a stick pushed well off centre.
pub fn stick_direction(v: Vec2) -> Option<Dir> {
    if v.length() < 0.6 {
        return None;
    }
    Some(if v.x.abs() > v.y.abs() {
        if v.x > 0.0 { Dir::Right } else { Dir::Left }
    } else if v.y > 0.0 {
        Dir::Up
    } else {
        Dir::Down
    })
}

/// Where keyboard navigation applies right now.
fn nav_active(
    state: Option<Res<State<AppState>>>,
    mode: Option<Res<WorldsScreenMode>>,
    paused: Option<Res<PauseState>>,
    inventory: Option<Res<InventoryState>>,
    knap: Option<Res<KnapState>>,
    death: Option<Res<DeathState>>,
) -> bool {
    let Some(state) = state else { return false };
    match state.get() {
        AppState::MainMenu | AppState::Mods => true,
        AppState::Worlds => mode.is_none_or(|m| *m != WorldsScreenMode::Create),
        AppState::InGame => paused.is_some_and(|p| p.open) || inventory.is_some_and(|i| i.open) || knap.is_some_and(|k| k.is_open()) || death.is_some_and(|d| d.open),
        AppState::Settings => false,
    }
}

pub(crate) fn menu_nav_for_tests(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pad: Option<Res<PadState>>,
    focus: ResMut<MenuFocus>,
    buttons: Query<(Entity, &GlobalTransform, &ComputedNode, Mut<Interaction>), With<Button>>,
) {
    menu_nav(keys, pad, focus, buttons);
}

fn menu_nav(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pad: Option<Res<PadState>>,
    mut focus: ResMut<MenuFocus>,
    mut buttons: Query<(Entity, &GlobalTransform, &ComputedNode, Mut<Interaction>), With<Button>>,
) {
    // Release what we pressed last frame.
    if let Some(e) = focus.pressed.take() {
        if let Ok((.., mut i)) = buttons.get_mut(e) {
            *i = Interaction::None;
        }
    }
    focus.secondary = false;
    // The mouse arriving on a button takes over from the keyboard.
    let focused = focus.entity;
    if buttons.iter().any(|(e, _, _, i)| i.is_changed() && *i == Interaction::Hovered && Some(e) != focused) {
        focus.entity = None;
        focus.restore_in = 0;
    }
    // A highlighted button that no longer exists (the screen was rebuilt - the inventory
    // does that after every click): remember where it was and re-highlight whatever is
    // there once the new screen has been laid out.
    if focus.entity.is_some_and(|e| buttons.get(e).is_err()) {
        focus.entity = None;
        focus.restore_in = RESTORE_FRAMES;
    }
    if let Some(e) = focus.entity {
        if let Ok((_, t, ..)) = buttons.get(e) {
            focus.last_center = Some(t.translation().truncate());
        }
    }
    if focus.restore_in > 0 && focus.entity.is_none() {
        focus.restore_in -= 1;
        if focus.restore_in == 0 {
            if let Some(at) = focus.last_center {
                let list: Vec<(Entity, Vec2)> = buttons.iter().map(|(e, t, ..)| (e, t.translation().truncate())).collect();
                focus.entity = nearest_to(&list.iter().map(|(_, c)| *c).collect::<Vec<_>>(), at).map(|i| list[i].0);
            }
        }
    }
    let stick = pad.as_deref().and_then(|p| stick_direction(p.left));
    let flick = if stick.is_some() && stick != focus.stick { stick } else { None };
    focus.stick = stick;
    let pad_dir = pad.as_deref().and_then(pad_direction).or(flick);
    let pad_confirm = pad.as_deref().is_some_and(|p| p.started(PadButton::South));
    let pad_secondary = pad.as_deref().is_some_and(|p| p.started(PadButton::West));
    let keys_default = ButtonInput::<KeyCode>::default();
    let keys = keys.as_deref().unwrap_or(&keys_default);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let dir = if keys.just_pressed(KeyCode::ArrowDown) {
        Some(Dir::Down)
    } else if keys.just_pressed(KeyCode::ArrowUp) {
        Some(Dir::Up)
    } else if keys.just_pressed(KeyCode::ArrowRight) {
        Some(Dir::Right)
    } else if keys.just_pressed(KeyCode::ArrowLeft) {
        Some(Dir::Left)
    } else if keys.just_pressed(KeyCode::Tab) {
        Some(if shift { Dir::Prev } else { Dir::Next })
    } else {
        None
    };
    let dir = dir.or(pad_dir);
    if let Some(dir) = dir {
        let list: Vec<(Entity, Spot)> = buttons
            .iter()
            .map(|(e, t, n, _)| (e, Spot { center: t.translation().truncate(), size: n.size() }))
            .collect();
        let spots: Vec<Spot> = list.iter().map(|(_, s)| *s).collect();
        let current = focus.entity.and_then(|f| list.iter().position(|(e, _)| *e == f));
        if let Some(i) = navigate(&spots, current, dir) {
            focus.entity = Some(list[i].0);
        }
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) || pad_confirm || pad_secondary {
        focus.secondary = pad_secondary && !pad_confirm;
        if let Some(e) = focus.entity {
            if let Ok((.., mut i)) = buttons.get_mut(e) {
                *i = Interaction::Pressed;
                focus.pressed = Some(e);
            }
        }
    }
    // Remember where the highlight ended up *this* frame, so a rebuild right after a
    // move still restores to the right place.
    if let Some(e) = focus.entity {
        if let Ok((_, t, ..)) = buttons.get(e) {
            focus.last_center = Some(t.translation().truncate());
        }
    }
}

/// Keeps the highlighted button visible: if it sits outside the scrolling list that holds it,
/// scrolls that list just enough. (Only the highlight scrolls; the mouse wheel is unchanged.)
fn scroll_focus_into_view(
    focus: Res<MenuFocus>,
    nodes: Query<(&GlobalTransform, &ComputedNode)>,
    parents: Query<&ChildOf>,
    mut lists: Query<&mut ScrollPosition>,
) {
    let Some(entity) = focus.entity else { return };
    let Ok((gt, node)) = nodes.get(entity) else { return };
    let (centre, half) = (gt.translation().y, node.size().y * 0.5);
    let mut at = entity;
    while let Ok(parent) = parents.get(at) {
        at = parent.parent();
        let Ok(mut scroll) = lists.get_mut(at) else { continue };
        let Ok((lgt, lnode)) = nodes.get(at) else { continue };
        let (lc, lh) = (lgt.translation().y, lnode.size().y * 0.5);
        let above = (lc - lh) - (centre - half);
        let below = (centre + half) - (lc + lh);
        let k = lnode.inverse_scale_factor();
        if above > 0.5 {
            scroll.offset_y -= above * k;
        } else if below > 0.5 {
            scroll.offset_y += below * k;
        }
        break;
    }
}

pub struct MenuNavPlugin;

impl Plugin for MenuNavPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuFocus>()
            .add_systems(PreUpdate, menu_nav.after(UiSystem::Focus).run_if(nav_active))
            .add_systems(Update, scroll_focus_into_view.run_if(nav_active));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(x: f32, y: f32) -> Spot {
        Spot { center: Vec2::new(x, y), size: Vec2::new(100.0, 40.0) }
    }

    // A column of three, then a row of two:   A / B / C / D E
    fn layout() -> Vec<Spot> {
        vec![spot(300.0, 100.0), spot(300.0, 160.0), spot(300.0, 220.0), spot(240.0, 300.0), spot(360.0, 300.0)]
    }

    #[test]
    fn with_nothing_highlighted_a_key_picks_the_first_or_last() {
        let l = layout();
        assert_eq!(navigate(&l, None, Dir::Down), Some(0));
        assert_eq!(navigate(&l, None, Dir::Next), Some(0));
        assert_eq!(navigate(&l, None, Dir::Up), Some(4));
        assert_eq!(navigate(&l, None, Dir::Prev), Some(4));
        assert_eq!(navigate(&[], None, Dir::Down), None);
    }

    #[test]
    fn up_and_down_walk_the_column_and_wrap() {
        let l = layout();
        assert_eq!(navigate(&l, Some(0), Dir::Down), Some(1));
        assert_eq!(navigate(&l, Some(1), Dir::Down), Some(2));
        assert_eq!(navigate(&l, Some(1), Dir::Up), Some(0));
        assert_eq!(navigate(&l, Some(0), Dir::Up), Some(4), "up from the top wraps to the end");
        assert_eq!(navigate(&l, Some(4), Dir::Down), Some(0), "down from the bottom wraps to the start");
    }

    #[test]
    fn left_and_right_move_along_a_row_and_up_down_leave_it() {
        let l = layout();
        assert_eq!(navigate(&l, Some(3), Dir::Right), Some(4));
        assert_eq!(navigate(&l, Some(4), Dir::Left), Some(3));
        assert_eq!(navigate(&l, Some(2), Dir::Down), Some(3), "down from C reaches the row below (nearest in x)");
        assert_eq!(navigate(&l, Some(4), Dir::Up), Some(2));
    }

    #[test]
    fn tab_follows_reading_order_and_wraps() {
        let l = layout();
        assert_eq!(navigate(&l, Some(2), Dir::Next), Some(3));
        assert_eq!(navigate(&l, Some(3), Dir::Next), Some(4));
        assert_eq!(navigate(&l, Some(4), Dir::Next), Some(0));
        assert_eq!(navigate(&l, Some(0), Dir::Prev), Some(4));
    }

    #[test]
    fn a_lone_button_stays_put() {
        let l = vec![spot(10.0, 10.0)];
        for d in [Dir::Up, Dir::Down, Dir::Left, Dir::Right, Dir::Next, Dir::Prev] {
            assert_eq!(navigate(&l, Some(0), d), Some(0), "{d:?}");
        }
    }

    #[test]
    fn a_stale_highlight_index_is_treated_as_none() {
        assert_eq!(navigate(&layout(), Some(99), Dir::Down), Some(0));
    }

    #[test]
    fn the_stick_picks_its_dominant_direction_past_a_threshold() {
        assert_eq!(stick_direction(Vec2::new(0.2, 0.3)), None);
        assert_eq!(stick_direction(Vec2::new(0.0, 0.9)), Some(Dir::Up), "stick up is +y, like the left-stick movement");
        assert_eq!(stick_direction(Vec2::new(0.0, -0.9)), Some(Dir::Down));
        assert_eq!(stick_direction(Vec2::new(-0.9, 0.3)), Some(Dir::Left));
        assert_eq!(stick_direction(Vec2::new(0.7, -0.5)), Some(Dir::Right));
    }

    /// The whole path through Bevy: keys move the highlight, Enter presses for one frame.
    #[test]
    fn keys_highlight_and_press_a_real_button() {
        use crate::ui_theme::{ButtonStyle, ThemedButton};
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<MenuFocus>().add_systems(Update, menu_nav);
        let mk = |app: &mut App, y: f32| {
            app.world_mut()
                .spawn((
                    Button,
                    ThemedButton { style: ButtonStyle::Normal, selected: false, accent: None },
                    GlobalTransform::from_xyz(100.0, y, 0.0),
                    ComputedNode { size: Vec2::new(100.0, 40.0), ..default() },
                ))
                .id()
        };
        let (a, b) = (mk(&mut app, 100.0), mk(&mut app, 160.0));
        let press = |app: &mut App, k: KeyCode| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(k);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(k);
            keys.clear();
        };
        press(&mut app, KeyCode::ArrowDown);
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(a));
        press(&mut app, KeyCode::ArrowDown);
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(b));
        press(&mut app, KeyCode::Enter);
        assert_eq!(*app.world().get::<Interaction>(b).unwrap(), Interaction::Pressed);
        app.update();
        assert_eq!(*app.world().get::<Interaction>(b).unwrap(), Interaction::None, "pressed for one frame only");
        // Moving the mouse onto the other button takes the highlight away.
        *app.world_mut().get_mut::<Interaction>(a).unwrap() = Interaction::Hovered;
        app.update();
        assert_eq!(app.world().resource::<MenuFocus>().entity, None);
    }

    #[test]
    fn the_dpad_and_a_button_drive_the_menu_like_the_keyboard() {
        use crate::ui_theme::{ButtonStyle, ThemedButton};
        let mut app = App::new();
        app.init_resource::<MenuFocus>().init_resource::<PadState>().add_systems(Update, menu_nav);
        let mk = |app: &mut App, y: f32| {
            app.world_mut()
                .spawn((
                    Button,
                    ThemedButton { style: ButtonStyle::Normal, selected: false, accent: None },
                    GlobalTransform::from_xyz(100.0, y, 0.0),
                    ComputedNode { size: Vec2::new(100.0, 40.0), ..default() },
                ))
                .id()
        };
        let (a, b) = (mk(&mut app, 100.0), mk(&mut app, 160.0));
        let tap = |app: &mut App, button: PadButton| {
            let mut p = PadState::default();
            p.just_pressed.insert(button);
            p.pressed.insert(button);
            *app.world_mut().resource_mut::<PadState>() = p;
            app.update();
            *app.world_mut().resource_mut::<PadState>() = PadState::default();
        };
        tap(&mut app, PadButton::DPadDown);
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(a));
        tap(&mut app, PadButton::DPadDown);
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(b));
        tap(&mut app, PadButton::South);
        assert_eq!(*app.world().get::<Interaction>(b).unwrap(), Interaction::Pressed, "A presses it");
        // A flick of the left stick moves once, however long it is held.
        app.update();
        let stick = |app: &mut App, v: Vec2| {
            app.world_mut().resource_mut::<PadState>().left = v;
            app.update();
        };
        stick(&mut app, Vec2::new(0.0, 0.9));
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(a), "stick up moved up");
        stick(&mut app, Vec2::new(0.0, 0.9));
        stick(&mut app, Vec2::new(0.0, 0.9));
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(a), "held, it doesn't keep going");
        stick(&mut app, Vec2::ZERO);
        stick(&mut app, Vec2::new(0.0, -0.9));
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(b), "a second flick moves again");
    }

    fn nav_app() -> App {
        let mut app = App::new();
        app.init_resource::<MenuFocus>().init_resource::<PadState>().add_systems(Update, (menu_nav, scroll_focus_into_view).chain());
        app
    }

    fn button_at(app: &mut App, x: f32, y: f32) -> Entity {
        app.world_mut()
            .spawn((Button, GlobalTransform::from_xyz(x, y, 0.0), ComputedNode { size: Vec2::new(40.0, 40.0), inverse_scale_factor: 1.0, ..default() }))
            .id()
    }

    fn tap(app: &mut App, b: PadButton) {
        let mut p = PadState::default();
        p.just_pressed.insert(b);
        p.pressed.insert(b);
        *app.world_mut().resource_mut::<PadState>() = p;
        app.update();
        *app.world_mut().resource_mut::<PadState>() = PadState::default();
    }

    #[test]
    fn the_highlight_comes_back_where_it_was_when_the_screen_is_rebuilt() {
        let mut app = nav_app();
        let _a = button_at(&mut app, 100.0, 100.0);
        let b = button_at(&mut app, 100.0, 160.0);
        tap(&mut app, PadButton::DPadDown);
        tap(&mut app, PadButton::DPadDown);
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(b));
        // The inventory rebuilds itself after every click: same layout, new entities.
        app.world_mut().despawn(b);
        let a2 = button_at(&mut app, 100.0, 100.0);
        let b2 = button_at(&mut app, 100.0, 160.0);
        assert_ne!(b, b2);
        for _ in 0..(RESTORE_FRAMES + 1) {
            app.update();
        }
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(b2), "highlight returned to the same spot, not the first button ({a2:?})");
    }

    #[test]
    fn x_is_a_secondary_press_for_one_frame_and_a_is_not() {
        let mut app = nav_app();
        let b = button_at(&mut app, 100.0, 100.0);
        tap(&mut app, PadButton::DPadDown);
        tap(&mut app, PadButton::West);
        assert!(app.world().resource::<MenuFocus>().secondary);
        assert_eq!(*app.world().get::<Interaction>(b).unwrap(), Interaction::Pressed);
        app.update();
        assert!(!app.world().resource::<MenuFocus>().secondary, "only for the frame it was pressed");
        tap(&mut app, PadButton::South);
        assert!(!app.world().resource::<MenuFocus>().secondary, "A is the plain press");
    }

    #[test]
    fn moving_the_highlight_scrolls_a_list_to_keep_it_in_view() {
        let mut app = nav_app();
        // A 100px-tall list centred at y=100 (50..150), scrolled to the top.
        let list = app
            .world_mut()
            .spawn((GlobalTransform::from_xyz(100.0, 100.0, 0.0), ComputedNode { size: Vec2::new(200.0, 100.0), inverse_scale_factor: 1.0, ..default() }, ScrollPosition::default()))
            .id();
        let near = button_at(&mut app, 100.0, 80.0);
        let far = button_at(&mut app, 100.0, 300.0); // far below the list's bottom edge
        app.world_mut().entity_mut(near).insert(ChildOf(list));
        app.world_mut().entity_mut(far).insert(ChildOf(list));
        tap(&mut app, PadButton::DPadDown);
        assert_eq!(app.world().get::<ScrollPosition>(list).unwrap().offset_y, 0.0, "already visible: no scrolling");
        tap(&mut app, PadButton::DPadDown);
        assert_eq!(app.world().resource::<MenuFocus>().entity, Some(far));
        let y = app.world().get::<ScrollPosition>(list).unwrap().offset_y;
        assert!(y > 100.0, "scrolled down to reach it: {y}");
    }

    #[test]
    fn nearest_to_picks_the_closest_point() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 90.0)];
        assert_eq!(nearest_to(&pts, Vec2::new(95.0, 80.0)), Some(2));
        assert_eq!(nearest_to(&[], Vec2::ZERO), None);
    }
}
