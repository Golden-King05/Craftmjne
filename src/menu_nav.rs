//! Keyboard navigation for the point-and-click menus: arrow keys (or Tab / Shift+Tab)
//! move a highlight between buttons, Enter presses the highlighted one.
//!
//! The highlight is [`MenuFocus`]; `ui_theme` draws a focused button with its hover
//! texture, so it looks exactly like the mouse is on it. Pressing is done by setting the
//! button's `Interaction` to `Pressed` for one frame, which is what every screen's own
//! click handler already reacts to - no screen needed any change. Moving the mouse onto a
//! button drops the keyboard highlight so the two never disagree.
//!
//! It runs only where it can't fight something else for the keys: the main menu, the mods
//! list, the worlds list and world options, and the pause menu. The create-world form
//! (typing) and Settings (typing into search, capturing a key to bind) are left alone.
//! The keys here are fixed, not rebindable controls: menus have to work before anyone has
//! touched the controls screen.

use bevy::prelude::*;
use bevy::ui::UiSystem;

use crate::menu::WorldsScreenMode;
use crate::state::{AppState, PauseState};
use crate::ui_theme::ThemedButton;

/// Which button the keyboard has highlighted.
#[derive(Resource, Default, Debug)]
pub struct MenuFocus {
    pub entity: Option<Entity>,
    /// A button this system pressed last frame; released at the start of the next.
    pressed: Option<Entity>,
}

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

/// Where keyboard navigation applies right now.
fn nav_active(state: Option<Res<State<AppState>>>, mode: Option<Res<WorldsScreenMode>>, paused: Option<Res<PauseState>>) -> bool {
    let Some(state) = state else { return false };
    match state.get() {
        AppState::MainMenu | AppState::Mods => true,
        AppState::Worlds => mode.is_none_or(|m| *m != WorldsScreenMode::Create),
        AppState::InGame => paused.is_some_and(|p| p.open),
        AppState::Settings => false,
    }
}

fn menu_nav(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut focus: ResMut<MenuFocus>,
    mut buttons: Query<(Entity, &GlobalTransform, &ComputedNode, Mut<Interaction>), With<ThemedButton>>,
) {
    // Release what we pressed last frame.
    if let Some(e) = focus.pressed.take() {
        if let Ok((.., mut i)) = buttons.get_mut(e) {
            *i = Interaction::None;
        }
    }
    // The mouse arriving on a button takes over from the keyboard.
    let focused = focus.entity;
    if buttons.iter().any(|(e, _, _, i)| i.is_changed() && *i == Interaction::Hovered && Some(e) != focused) {
        focus.entity = None;
    }
    // A highlighted button that no longer exists (the screen was rebuilt).
    if focus.entity.is_some_and(|e| buttons.get(e).is_err()) {
        focus.entity = None;
    }
    let Some(keys) = keys else { return };
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
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        if let Some(e) = focus.entity {
            if let Ok((.., mut i)) = buttons.get_mut(e) {
                *i = Interaction::Pressed;
                focus.pressed = Some(e);
            }
        }
    }
}

pub struct MenuNavPlugin;

impl Plugin for MenuNavPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuFocus>()
            .add_systems(PreUpdate, menu_nav.after(UiSystem::Focus).run_if(nav_active));
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
}
