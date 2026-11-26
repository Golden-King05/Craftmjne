//! Craftmjne — a well-optimized, extensible voxel game framework on Bevy.
//!
//! Framework usage: everything is a Bevy plugin. Add your own plugins next to
//! the built-in ones; register blocks/painters from your plugin's `build()`
//! (see README "Extending the framework").
//!
//! CLI: `craftmjne [--seed N] [--render-distance N] [--instance ID] [--version]`
//!
//! There is no update flag any more: versions are managed by the launcher
//! (`launcher/`), which installs each build into its own folder rather than
//! having the game rewrite itself.

// Without this a Windows release build pops a console window behind the
// game every single launch - nothing here writes enough to stdout/stderr in
// normal play to be worth that. Debug builds keep it, since that's where a
// panic message is worth seeing. Same fix, same reasoning, as the launcher's
// `launcher/src/main.rs`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bevy::prelude::*;

use craftmjne::config::WorldSettings;
use craftmjne::save::{GameMode, SaveStore};
use craftmjne::state::{ActiveWorld, AppState};
use craftmjne::{block_models, chat, commands, damage, health, interact, entities, inventory, items, knap_ui, light, menu, particles, player, render, sky, states, ui, world};

struct Args {
    seed: u32,
    render_distance: Option<i32>, // CLI override; None means "use the saved graphics setting"
    /// Set by the launcher: which of its instances this is. Controls are saved per
    /// instance, so they follow it through a rename or a game update.
    instance: Option<String>,
}

fn parse_args() -> Args {
    let mut seed = WorldSettings::default().seed;
    let mut render_distance = None;
    let mut instance = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" | "-s" => {
                if let Some(v) = args.next().and_then(|v| v.parse().ok()) {
                    seed = v;
                }
            }
            "--render-distance" | "-r" => {
                if let Some(v) = args.next().and_then(|v| v.parse().ok()) {
                    render_distance = Some(v);
                }
            }
            "--instance" => instance = args.next(),
            "--version" | "-V" => {
                println!("craftmjne {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => eprintln!("unknown argument: {other}"),
        }
    }
    Args { seed, render_distance, instance }
}

/// Reconnects stdout/stderr to whatever terminal launched this process, if
/// any. `windows_subsystem = "windows"` starts with no console at all, so
/// even `craftmjne --version` run from an open PowerShell would otherwise
/// print nothing; `AttachConsole` with the parent-process pseudo-handle
/// borrows that terminal's console when one exists, and fails harmlessly
/// (nothing to attach to) on a plain double-click. Same fix as the
/// launcher's `launcher/src/diagnostics.rs::attach_console`.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: no arguments to get wrong; documented to fail cleanly when the
    // process has no parent console.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}

fn main() {
    attach_console();
    let args = parse_args();
    let smoke = std::env::var_os("CRAFT_SMOKE").is_some();
    // UI preview mode: `CRAFT_SHOT=<screen> CRAFT_SHOT_OUT=<file.png> craftmjne` opens one
    // screen over a throwaway save folder, saves a screenshot and exits. See `ui_shot`.
    let shot = std::env::var("CRAFT_SHOT").ok();
    let smoke_like = smoke || shot.is_some();

    // CI / smoke mode: redirect saves to a throwaway directory so automated
    // runs never touch (or depend on) a real user profile.
    let store = if smoke_like {
        let dir = std::env::temp_dir().join(format!("craftmjne-smoke-{}", std::process::id()));
        SaveStore::at(dir)
    } else {
        SaveStore::default()
    }
    .with_instance(args.instance.clone());

    let graphics = store.load_graphics_settings();
    let settings = WorldSettings {
        seed: args.seed,
        render_distance: args.render_distance.unwrap_or(graphics.render_distance),
    };

    let mut app = App::new();
    app.insert_resource(settings)
        .insert_resource(store)
        .add_plugins(
            // Bevy's default window handling is back: nothing needs to
            // intercept the close button any more, now that quitting no
            // longer has to give a staged self-update a chance to apply.
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Craftmjne".into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .init_state::<AppState>()
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            world::WorldPlugin,
            render::RenderSetupPlugin,
            player::PlayerPlugin,
            light::LightPlugin,
            sky::SkyPlugin,
            interact::InteractPlugin,
            inventory::InventoryPlugin,
            knap_ui::KnapUiPlugin,
            items::ItemsPlugin,
            health::HealthPlugin,
            commands::CommandsPlugin,
            chat::ChatPlugin,
            ui::UiPlugin,
            menu::MenuPlugin,
        ))
        .add_plugins((entities::EntitiesPlugin, block_models::BlockModelsPlugin, particles::ParticlesPlugin, damage::DamagePlugin, states::StatesPlugin))
        .add_plugins((craftmjne::packs::PackPlugin, craftmjne::pack_panel::PackPanelPlugin, craftmjne::fonts::FontPlugin, craftmjne::keybinds::KeybindPlugin, craftmjne::mod_controls::ModControlsPlugin, craftmjne::settings::SettingsPlugin, craftmjne::sound::SoundPlugin));

    // CI / smoke mode: skip the menu, drop straight into a throwaway world,
    // let it stream in, save a screenshot, and exit. Run with:
    // CRAFT_SMOKE=1 cargo run -- --seed 7
    if let Some(name) = shot {
        for (i, world) in ["Survival Island", "Creative Sandbox", "Modded Test"].iter().enumerate() {
            let store = app.world().resource::<SaveStore>().clone();
            let _ = store.create_world(world, 100 + i as u32, if i == 1 { GameMode::Creative } else { GameMode::Survival });
        }
        if name == "pause" || name == "game" || name == "inventory" {
            app.add_systems(Update, enter_smoke_world.run_if(in_state(AppState::MainMenu)));
        }
        // `CRAFT_CJK=ja|zh-Hans|zh-Hant` picks the regional forms of the Chinese/Japanese screen.
        if let Some(style) = std::env::var("CRAFT_CJK").ok().and_then(|l| craftmjne::cjk::Style::from_language(&l)) {
            app.insert_resource(craftmjne::cjk::CjkStyle(style));
        }
        app.insert_resource(ShotScreen(name)).add_systems(Update, ui_shot);
    }
    if smoke {
        app.add_systems(Update, enter_smoke_world.run_if(in_state(AppState::MainMenu)))
            .add_systems(Update, smoke_test);
    }

    app.run();
}

/// Runs only while still in `MainMenu`, so it fires exactly once: creates a
/// throwaway world and jumps straight into it, skipping the menu entirely.
fn enter_smoke_world(
    mut commands: Commands,
    settings: Res<WorldSettings>,
    store: Res<SaveStore>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let (slug, meta) = store
        .create_world("smoke-test", settings.seed, GameMode::default())
        .expect("create smoke-test world");
    commands.insert_resource(ActiveWorld { slug, meta });
    next_state.set(AppState::InGame);
}

fn smoke_test(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: Local<u32>,
    mut exit: EventWriter<AppExit>,
    map: Res<craftmjne::world::ChunkMap>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let t = time.elapsed_secs();
    if *phase == 0 && t > 20.0 {
        let (generated, meshed) = map.stats();
        info!("smoke: {generated} chunks generated, {meshed} meshed; capturing smoke.png");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("smoke.png"));
        *phase = 1;
    } else if *phase == 1 && t > 24.0 {
        info!("smoke: done");
        exit.write(AppExit::Success);
    }
}

/// Which screen `CRAFT_SHOT` asked for.
#[derive(Resource)]
struct ShotScreen(String);

/// Opens the requested screen, waits for it to draw, saves a screenshot and exits.
/// Screens: `main`, `worlds`, `create`, `mods`, `settings-<player|rendering|sound|
/// controls|options|mods>`, `pause`, and `game` (the world with nothing over it). There is no other way to look at the UI from a
/// machine with no screen, so this is how menu changes get checked by eye.
fn ui_shot(
    mut commands: Commands,
    time: Res<Time>,
    shot: Res<ShotScreen>,
    mut phase: Local<u32>,
    mut next: ResMut<NextState<AppState>>,
    mut worlds_mode: ResMut<craftmjne::menu::WorldsScreenMode>,
    mut view: ResMut<craftmjne::settings::SettingsView>,
    mut paused: ResMut<craftmjne::state::PauseState>,
    mut inventory: ResMut<craftmjne::inventory::InventoryState>,
    mut focus: ResMut<craftmjne::menu_nav::MenuFocus>,
    buttons: Query<(Entity, &GlobalTransform), With<Button>>,
    buttons_roots: Query<Entity, With<craftmjne::menu::MainMenuRoot>>,
    mut exit: EventWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    use craftmjne::settings::{ControlsTab, Section};
    let t = time.elapsed_secs();
    let name = shot.0.as_str();
    let in_game = name == "pause" || name == "game" || name == "inventory";
    let out = std::env::var("CRAFT_SHOT_OUT").unwrap_or_else(|_| "shot.png".to_string());
    match *phase {
        0 if t > 1.0 => {
            match name {
                "worlds" | "create" | "world-options" => next.set(AppState::Worlds),
                "mods" => next.set(AppState::Mods),
                n if n.starts_with("settings") => next.set(AppState::Settings),
                _ => {}
            }
            *phase = 1;
        }
        1 if t > 3.0 => {
            match name {
                "create" => *worlds_mode = craftmjne::menu::WorldsScreenMode::Create,
                "world-options" => *worlds_mode = craftmjne::menu::WorldsScreenMode::Options,
                "fonts" | "scripts" | "cjk" | "natives" | "world" | "asia1" | "asia2" | "mongol" | "joined" => {
                    let scripts = name == "scripts";
                    let cjk = name == "cjk";
                    let natives = name == "natives";
                    let world = name == "world";
                    let asia1 = name == "asia1";
                    let asia2 = name == "asia2";
                    let mongol = name == "mongol";
                    let joined = name == "joined";
                    for e in &buttons_roots {
                        commands.entity(e).despawn();
                    }
                    commands.spawn((
                        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(24.0)), row_gap: Val::Px(6.0), ..default() },
                        BackgroundColor(Color::srgb(0.16, 0.12, 0.09)),
                    ))
                    .with_children(|p| {
                        let sample: &[(f32, &str)] = if joined {
                            &[
                                (24.0, "Arabic:  مرحبا بالعالم السلام عليكم كيف حالك  لا"),
                                (24.0, "Persian: پارسی چای ژاله گل کتاب فارسی  گچپژ"),
                                (24.0, "Syriac:  ܣܘܪܝܝܐ ܫܠܡܐ ܒܝܬ ܡܠܟܐ ܐܠܗܐ ܟܬܒܐ ܒܪܝܫܝܬ"),
                                (24.0, "N'Ko:    ߒߞߏ ߞߊ߲ ߘߋ߲ ߛߓߍ ߝߊ߲ߠߊ ߊߋߌߍߎߏ"),
                                (24.0, "Mandaic: ࡌࡀࡍࡃࡀࡉࡀ ࡁࡂࡃࡄࡅࡆࡇࡈࡉ ࡌࡍࡎࡏ"),
                                (24.0, "Mongol:  ᠮᠣᠩᠭᠣᠯ ᠤᠯᠤᠰ ᠪᠢᠴᠢᠭ ᠮᠠᠨᠵᡠ ᡤᡳᠰᡠᠨ ᠲᠣᠳᠣ"),
                                (24.0, "Hindi:   क्षत्रिय ज्ञान श्री प्रेम धर्म कर्म क्या"),
                                (24.0, "Mixed: hello مرحبا ܣܘܪܝܝܐ world ᠮᠣᠩᠭᠣᠯ 123"),
                                (12.0, "مرحبا ܣܘܪܝܝܐ ߒߞߏ ࡌࡀࡍ ᠮᠣᠩᠭᠣᠯ क्षत्रिय (twelve)"),
                            ]
                        } else if mongol {
                            &[
                                (24.0, "Before (separate letters, left to right):  ᠮᠣᠩᠭᠣᠯ ᠤᠯᠤᠰ ᠪᠢᠴᠢᠭ"),
                                (24.0, "After (joined, upright, columns left to right) ->"),
                            ]
                        } else if asia1 {
                            &[
                                (24.0, "Монгол Улс, Улаанбаатар. Өнөөдөр сайхан үдэш. ӨөҮү ҢңҺһ"),
                                (24.0, "ᠮᠣᠩᠭᠣᠯ ᠤᠯᠤᠰ ᠪᠢᠴᠢᠭ ᠮᠠᠨᠵᡠ ᡤᡳᠰᡠᠨ ᠲᠣᠳᠣ ꡖꡍꡂꡛ ꡏꡡꡃꡡꡙ"),
                                (24.0, "ગુજરાતી ભાષા કેમ છો? નમસ્તે  ਪੰਜਾਬੀ ਭਾਸ਼ਾ ਸਤ ਸ੍ਰੀ ਅਕਾਲ"),
                                (24.0, "বাংলা ভাষা আমি বাংলায় গান গাই। অসমীয়া  தமிழ் மொழி வணக்கம் உலகம்"),
                                (24.0, "ಕನ್ನಡ ಭಾಷೆ ನಮಸ್ಕಾರ ಜಗತ್ತು  മലയാളം ഭാഷ നമസ്കാരം ലോകം"),
                                (24.0, "සිංහල භාෂාව ආයුබෝවන් ලෝකය  తెలుగు భాష నమస్కారం ప్రపంచం"),
                                (24.0, "ଓଡ଼ିଆ ଭାଷା ନମସ୍କାର  ދިވެހި ބަސް ހެޔޮ ދުވަހަކަށް"),
                                (24.0, "བོད་སྐད། བཀྲ་ཤིས་བདེ་ལེགས། སངས་རྒྱས། ཨོཾ་མ་ཎི་པདྨེ་ཧཱུྃ"),
                                (12.0, "Монгол ᠮᠣᠩᠭᠣᠯ ગુજરાતી ਪੰਜਾਬੀ বাংলা தமிழ் ಕನ್ನಡ മലയാളം සිංහල བོད་སྐད (twelve)"),
                            ]
                        } else if asia2 {
                            &[
                                (24.0, "ᱥᱟᱱᱛᱟᱲᱤ ᱯᱟᱹᱛᱷᱟᱲ ᱚᱞ ᱪᱤᱠᱤ  မြန်မာဘာသာ မင်္ဂလာပါ ကမ္ဘာ"),
                                (24.0, "ພາສາລາວ ສະບາຍດີ ໂລກ  ภาษาไทย สวัสดีชาวโลก ผู้ใหญ่ น้ำ"),
                                (24.0, "ភាសាខ្មែរ សួស្តីពិភពលោក ក្រុង  Tiếng Việt: Xin chào thế giới!"),
                                (24.0, "Nguyễn Thị Hương, Đà Nẵng, Hà Nội. ẤẦẨẪẬ ẮẰẲẴẶ ỂỆỐỒ ƠƯ ỵỹỷ"),
                                (24.0, "ᜊᜌ᜔ᜊᜌᜒᜈ᜔ ᜃᜈ᜔ᜆᜓ  ᜰᜲᜳ ᜱ  ᨒᨚᨈᨑ ᨅᨘᨁᨗᨔ  ᬩᬲᬩᬮᬶ ᬫᬦᬸᬲ"),
                                (24.0, "ꦧꦱꦗꦮ ꦱꦸꦩꦸꦮꦺ  ᮘᮞ ᮞᮥᮔ᮪ᮓ ᮎᮨᮔ᮪  ᯅᯖᯂ᯲ ᯖᯬᯅ"),
                                (24.0, "ꆈꌠꁱꂷ ꀀꀁꀂꀃ ꑍꆏ  𖼀𖼁𖼂 𖽐𖽑  𛅰𛅱𛅲 𛆀𛆁"),
                                (24.0, "ㄅㄆㄇㄈ ˊˇˋ˙ ㄓㄨㄥ ㄨㄣˊ ㄏㄢˋ ㄩˇ ㄓㄨˋ ㄧㄣ"),
                                (12.0, "ᱥᱟᱱᱛᱟᱲᱤ မြန်မာ ພາສາລາວ ไทย ខ្មែរ Việt ᜊᜌ᜔ᜊᜌᜒᜈ᜔ ᬩᬲᬩᬮᬶ ꆈꌠ (twelve)"),
                            ]
                        } else if world {
                            &[
                                (24.0, "Η γρήγορη καφέ αλεπού πηδά πάνω από το τεμπέλικο σκυλί. ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ ΐΰ"),
                                (24.0, "Ἐν ἀρχῇ ἦν ὁ λόγος, καὶ ὁ λόγος ἦν πρὸς τὸν θεόν · Ϙ Ϡ Ϟ"),
                                (24.0, "Հայաստան Երևան, Բարև աշխարհ։ ԱԲԳԴԵԶ աբգդեզ"),
                                (24.0, "საქართველო თბილისი გამარჯობა მსოფლიო ᲐᲑᲒ Ⴀ ⴀ"),
                                (24.0, "ܣܘܪܝܝܐ ܫܠܡܐ ܒܝܬ ܡܠܟܐ ܐܠܗܐ ܟܬܒܐ ܒܪܝܫܝܬ  ܫܠܵܡܵܐ"),
                                (24.0, "ࡌࡀࡍࡃࡀࡉࡀ ࡁࡂࡃࡄࡅࡆࡇࡈࡉ ࡌࡍࡎࡏ ࡀࡔࡅࡄ"),
                                (24.0, "Ⲙⲉⲧⲣⲉⲙⲛ̀ⲭⲏⲙⲓ ⲁⲃⲅⲇⲉⲋⲍⲏⲑ ⲀⲂⲄⲆ ϣϥϧϩϫϭϯ"),
                                (24.0, "አማርኛ ኢትዮጵያ ሰላም ለዓለም ሀሁሂሃሄህሆ ግዕዝ ቀዳማዊ"),
                                (24.0, "ⵜⴰⵎⴰⵣⵉⵖⵜ ⴰⵣⵓⵍ ⵜⵉⴼⵉⵏⴰⵖ ⴰⵎⴰⵣⵉⵖ ⵏ ⴻⵓⵜⵉⴼⵉⵏⴰⵖ ⵯ ⵿"),
                                (24.0, "ߒߞߏ ߞߊ߲ ߘߋ߲ ߛߓߍ ߝߊ߲ߠߊ ߊߋߌߍߎߏ ߀߁߂߃"),
                                (24.0, "ꕙꔤ ꗳ ꔔꘋ ꕉꕮ ꘉꘋꖸ ꔀꔁꔂ"),
                                (12.0, "Αβγ Ա Ⴀ ܐ ࡀ Ⲁ አ ⵜ ߊ ꔀ (twelve)"),
                            ]
                        } else if natives {
                            &[
                                (24.0, "ᏣᎳᎩ ᏓᎶᏂᎨᏍᏗ ᎠᏂᏴᏫᏯ ᎣᏏᏲ ᏙᎯᏧ  ꭰꭱꭲꭳꭴ ꮃꮄꮅ ᏗᎦᏘ"),
                                (24.0, "ᓀᐦᐃᔭᐍᐏᐣ ᐊᓯᔾᔨᐧᐃᐧᑭᓯᓯᐤ ᐱᐢᑯᓯ ᑭᑌᐧᐃᓄᐊ ᑲᐦᐋᐧᐃ  ᐊᒥᐢᑯᐧᐃᓂᐃᐧᑯᐤ"),
                                (24.0, "ᐃᓄᒃᑎᑐᑦ ᓄᓇᕗᑦ ᐅᖃᐅᓯᖅ ᖃᓄᐃᓕᐅᕈᑎᒃ ᑕᑯᔭᒐᓕᒋᐊᖅ ᐊᐃᑉᐹᐱ ᓇᑦᑎᒍ"),
                                (24.0, "Þú ert íslensk? Já, Ðór ætlar að fara í Öxará. Þórður, Ásgeir, Æsa, Ólöf, Hrafnhildur Ýr."),
                                (24.0, "Yupʼik: Yup\u{2BC}ik, cetaman, ḿikelnguq, ńukaq, qayaq, u\u{361}gaaq, ll rr ss gg ng ńg  Inuit: łŋ ŁŊ  Cree: nêhiyawêwin wîpâc"),
                                (12.0, "ᏣᎳᎩ ᐃᓄᒃᑎᑐᑦ ᓀᐦᐃᔭᐍᐏᐣ Þórður Yupʼik łŋ (twelve)"),
                            ]
                        } else if cjk {
                            &[
                                (24.0, "ひらがな あいうえお かきくけこ さしすせそ たちつてと なにぬねの はひふへほ まみむめも やゆよ らりるれろ わをん"),
                                (24.0, "カタカナ アイウエオ カキクケコ ガギグゲゴ パピプペポ ヴ ー ｱｲｳｴｵ ｶﾞ"),
                                (24.0, "日本語のテキスト。「こんにちは、世界！」 漢字仮名交じり文"),
                                (24.0, "直 骨 令 刃 海 角 糸 飲 食 語 話 読 鳥 魚 雪 愛 国 学 校"),
                                (24.0, "中文文本。你好，世界！简体字：汉语 学习 电脑 门 车 马 鸟 鱼"),
                                (24.0, "繁體字：漢語 學習 電腦 門 車 馬 鳥 魚 龍 龜 鬱 體 灣"),
                                (24.0, "ㄅㄆㄇㄈ ㄉㄊㄋㄌ ㄍㄎㄏ ㄐㄑㄒ ㄓㄔㄕㄖ ㄗㄘㄙ ㄚㄛㄜ ㄧㄨㄩ"),
                                (24.0, "nǐ hǎo, shìjiè  mā má mǎ mà  lǜ nǚ  ǖ ǘ ǚ ǜ  Tōkyō Ōsaka"),
                                (24.0, "〒〃々〆〇〈〉《》「」『』【】〔〕〜 ① ② ③ ㈱ ㌔ ㍿ ＡＢＣ１２３"),
                                (24.0, "※ → ← ↑ ↓ ⇒ ≠ ≦ ≧ ∞ ∴ ─│┌┐└┘ ■□●○◎▲△▼▽◆◇ ★☆♪♯♭ †‡…‥—“”‘’ ℃ № Σ π α β"),
                                (24.0, "鬱 龍 驫 麤 齉 灕 㐂 𠀀 𠮷 𪚥"),
                                (12.0, "日本語 中文 繁體 ひらがな カタカナ nǐ hǎo (twelve)"),
                            ]
                        } else if scripts {
                            &[
                                (24.0, "Привет, мир! Съешь ещё этих мягких булок"),
                                (24.0, "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ"),
                                (24.0, "абвгдеёжзийклмнопрстуфхцчшщъыьэюя ЇїЄєҐґ"),
                                (24.0, "שלום עולם אבגדהוזחטיכךלמםנןסעפףצץקרשת"),
                                (24.0, "안녕하세요 한국어 가나다라마바사 값 닭 읽 꽃"),
                                (24.0, "ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ ㅏㅑㅓㅕㅗㅛㅜㅠㅡㅣ ㅘㅝㅢ"),
                                (24.0, "مرحبا بالعالم السلام عليكم كيف حالك"),
                                (24.0, "ابتثجحخدذرزسشصضطظعغفقكلمنهوي ءأإآؤئة"),
                                (24.0, "नमस्ते दुनिया। हिन्दी भाषा कितनी सुंदर है"),
                                (24.0, "कखगघङचछजझञटठडढणतथदधनपफबभमयरलळवशषसह"),
                                (24.0, "का कि की कु कू कृ के कै को कौ कं कँ कः अआइईउऊऋएऐओऔ ०१२३"),
                                (24.0, "السَّلامُ عَلَيْكُمْ  لا  الله  كِتابٌ  ٠١٢٣٤٥٦٧٨٩"),
                                (24.0, "پارسی چای ژاله گل کتاب فارسی  ٹ ڈ ڑ  ۰۱۲۳۴۵۶۷۸۹"),
                                (24.0, "שָׁלוֹם עוֹלָם בְּרֵאשִׁית"),
                                (24.0, "क्ष त्र ज्ञ श्री क्या प्रेम धर्म कर्म पत्र क्लेश अग्नि"),
                                (12.0, "Привет שלום 안녕하세요 مرحبا नमस्ते (twelve)"),
                            ]
                        } else {
                            &[
                            (24.0, "The quick brown fox jumps over the lazy dog."),
                            (24.0, "THE QUICK BROWN FOX! 0123456789 (){}[]<>+-=*/@#$%&"),
                            (24.0, "àéîõü ÀÉÎÕÜ ñÑ çÇ ¡Hola! ¿Qué? ß æÆ øØ © € £ ~^`|_"),
                            (12.0, "The quick brown fox jumps over the lazy dog. (twelve)"),
                            (24.0, "😀😃😄😁😆😅🤣😂🙂😉😊😇🥰😍🤩😘😋😛🤔😎😴😭😱😡💀👻🤖"),
                            (24.0, "❤🧡💛💚💙💜🖤⭐✨🔥💧☀☁❄⚡🌈🌳🌵🍎🍌🍉🍕🍔🍪☕🍺"),
                            (24.0, "🐶🐱🐭🐰🦊🐻🐼🐷🐸🦁🐢🐝🦋🐙🐳🔑💣🎁🏆👑💎💡🚀🏠🕐🌑🌓🌕⬆➡"),
                            (24.0, "🇺🇸🇫🇷🇩🇪🇯🇵🇧🇷🇬🇧🇨🇦🇦🇺🇮🇳🇨🇳🇰🇷🇮🇹🇪🇸🇲🇽🇷🇺🇿🇦🇳🇬🇪🇬🇸🇦🇹🇷🇺🇦🇸🇪🇳🇴🇨🇭🇪🇺🇺🇳"),
                            (24.0, "\u{1F3F4}\u{E0075}\u{E0073}\u{E0074}\u{E0078}\u{E007F} \u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F} \u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F} \u{1F3F3}\u{FE0F}\u{200D}\u{1F308} \u{1F3F4}\u{200D}\u{2620}\u{FE0F} \u{1F3C1}\u{1F6A9}\u{1F38C} \u{1F52B}"),
                            (24.0, "👍🏻👍🏼👍🏽👍🏾👍🏿 👋🏽 1\u{FE0F}\u{20E3}2\u{FE0F}\u{20E3}3\u{20E3}#\u{FE0F}\u{20E3}*\u{FE0F}\u{20E3} ♈♉♊♋♌♍♎♏♐♑♒♓⛎"),
                            ]
                        };
                        if mongol {
                            p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(48.0), margin: UiRect::top(Val::Px(8.0)), ..default() }).with_children(|r| {
                                for (text, tint) in [
                                    ("ᠮᠣᠩᠭᠣᠯ ᠤᠯᠤᠰ ᠪᠢᠴᠢᠭ ᠮᠣᠩᠭᠣᠯ ᠤᠯᠤᠰ ᠪᠢᠴᠢᠭ ᠮᠣᠩᠭᠣᠯ", Color::WHITE),
                                    ("ᠮᠠᠨᠵᡠ ᡤᡳᠰᡠᠨ ᠮᠠᠨᠵᡠ ᡤᡳᠰᡠᠨ", Color::srgb(0.9, 0.8, 0.5)),
                                    ("ᠲᠣᠳᠣ ᠪᠢᠴᠢᠭ ᠲᠣᠳᠣ ᠪᠢᠴᠢᠭ ᠲᠣᠳᠣ", Color::srgb(0.6, 0.85, 1.0)),
                                ] {
                                    let mut e = r.spawn(craftmjne::mongolian::VerticalText::new(text, 300, 3));
                                    e.insert(ImageNode { color: tint, ..default() });
                                }
                            });
                        }
                        for (size, line) in sample.iter().copied() {
                            p.spawn((Text::new(line), TextFont { font_size: size, ..default() }, TextColor(Color::WHITE)));
                        }
                    });
                }
                "settings-rendering" => view.section = Section::Rendering,
                "settings-sound" => view.section = Section::Sound,
                "settings-mods" => view.section = Section::Mods,
                "settings-controls" => view.section = Section::Controls,
                "settings-controller" => {
                    view.section = Section::Controls;
                    view.device = craftmjne::keybinds::Device::Controller;
                    view.search = "btn:".into();
                }
                "settings-options" => {
                    view.section = Section::Controls;
                    view.controls_tab = ControlsTab::Options;
                }
                _ => {}
            }
            *phase = 2;
        }
        2 if t > if in_game { 14.0 } else { 5.0 } => {
            if name == "pause" {
                paused.open = true;
            }
            if name == "inventory" {
                inventory.open = true;
            }
            *phase = 3;
        }
        3 if name == "inventory" && focus.entity.is_none() && t > 15.0 => {
            // Highlight a slot as a controller would, so the screenshot shows it.
            let mut all: Vec<_> = buttons.iter().map(|(e, g)| (e, g.translation())).collect();
            all.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.1.x.total_cmp(&b.1.x)));
            focus.entity = all.get(12).map(|(e, _)| *e);
        }
        3 if t > if in_game { 16.0 } else { 7.0 } => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out));
            *phase = 4;
        }
        4 if t > if in_game { 19.0 } else { 10.0 } => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
