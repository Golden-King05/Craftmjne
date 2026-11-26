//! Faces: a head plus eyes, a mouth and extras, overlaid. Eyes sit on rows 3-5, the mouth on rows 7-10.

use super::{emoji, flip_vertical, overlay, parse, recolor, Emoji, Grid};

const DISC: &str = "....kkkk..../..kkyyyykk../.kyyyyyyyyk./.kyyyyyyyyk./kyyyyyyyyyyk/kyyyyyyyyyyk/kyyyyyyyyyyk/kyyyyyyyyyyk/.kyyyyyyyyk./.kyyyyyyyyk./..kkyyyykk../....kkkk....";
const CAT: &str = "kk........kk/kyk......kyk/kyykkkkkkyyk/kyyyyyyyyyyk/kyyyyyyyyyyk/kyyyyyyyyyyk/kyyyyyyyyyyk/kyyyyyyyyyyk/.kyyyyyyyyk./.kyyyyyyyyk./..kkyyyykk../....kkkk....";
const MONKEY: &str = "..kkk....kkk./.knnk.kknnk./knttnkknttnk/knnnnnnnnnnk/knnnnnnnnnnk/knnttttttnnk/knttttttttnk/knttttttttnk/.knttttttnk./.knttttttnk./..kkttttkk../....kkkk....";

type Overlay = (usize, &'static [&'static str]);

fn eyes(name: &str) -> Overlay {
    match name {
        "dot" => (4, &["...kk..kk...", "...kk..kk..."]),
        "wide" => (3, &["..kkk..kkk..", "..kwk..kwk..", "..kkk..kkk.."]),
        "happy" => (4, &["...k....k...", "..k.k..k.k.."]),
        "closed" => (5, &["..kkk..kkk.."]),
        "wink" => (4, &["...kk.......", "...kk..kkk.."]),
        "squint" => (3, &["..k......k..", "...k....k...", "..k......k.."]),
        "hearts" => (3, &["..r.r..r.r..", "..rrr..rrr..", "...r....r..."]),
        "stars" => (3, &["...o....o...", "..ooo..ooo..", "...o....o..."]),
        "cross" => (3, &["..k.k..k.k..", "...k....k...", "..k.k..k.k.."]),
        "shades" => (4, &[".kkkkkkkkkk.", "..kk....kk.."]),
        "up" => (3, &["..kwk..kwk..", "..kkk..kkk..", "..kkk..kkk.."]),
        "lids" => (4, &["..kkk..kkk..", "...kk..kk..."]),
        "nerd" => (3, &[".kkkk..kkkk.", ".kwwk..kwwk.", ".kwkk..kkwk.", ".kkkk..kkkk."]),
        "dollar" => (3, &["..ggg..ggg..", "..gGg..gGg..", "..ggg..ggg.."]),
        "panda" => (3, &["..kkk..kkk..", "..kwk..kwk..", "..kkk..kkk.."]),
        "none" => (0, &[]),
        other => panic!("no eyes called {other}"),
    }
}

fn mouth(name: &str) -> Overlay {
    match name {
        "smile" => (8, &["..k......k..", "...kkkkkk..."]),
        "grin" => (7, &["..kkkkkkkk..", "..kwwwwwwk..", "...kwwwwk...", "....kkkk...."]),
        "open" => (7, &["....kkkk....", "...kRRRRk...", "...kRRRRk...", "....kkkk...."]),
        "small" => (8, &[".....kk.....", ".....kk....."]),
        "flat" => (8, &["...kkkkkk..."]),
        "frown" => (8, &["...kkkkkk...", "..k......k.."]),
        "tongue" => (8, &["..k......k..", "...kkkkkk...", "....rrrr...."]),
        "tonguefl" => (8, &["...kkkkkk...", "....rrrr....", "....rrrr...."]),
        "kiss" => (8, &[".....rr.....", ".....rr....."]),
        "smirk" => (8, &["..........k.", "...kkkkkkk.."]),
        "zip" => (8, &[".kkkkkkkkkk.", "..k.k.k.k.k."]),
        "laugh" => (7, &["..kkkkkkkk..", "..kwwwwwwk..", "...kwrrwk...", "....kkkk...."]),
        "grimace" => (7, &["..kkkkkkkk..", "..kwkwkwwk..", "..kkkkkkkk.."]),
        "wavy" => (8, &["..k.k.k.k.k."]),
        "drool" => (8, &["..k......k..", "...kkkkkk...", ".......c....", ".......c...."]),
        "vomit" => (7, &["....kkkk....", "...kGGGGk...", "...kGGGGk...", "....kkkk...."]),
        "sadopen" => (8, &["....kkkk....", "...k....k...", "...k....k..."]),
        "scream" => (7, &["..kkkkkkkk..", "..kRRRRRRk..", "..kRRRRRRk..", "...kkkkkk..."]),
        "cry" => (8, &["..kkkkkkkk..", "..kRRRRRRk..", "...kkkkkk..."]),
        "teeth" => (8, &["..kkkkkkkk..", "..kwwwwwwk..", "..kkkkkkkk.."]),
        "none" => (0, &[]),
        other => panic!("no mouth called {other}"),
    }
}

fn extra(name: &str) -> Overlay {
    match name {
        "blush" => (6, &["..pp....pp.."]),
        "sweat" => (2, &["..........c.", ".........cc.", ".........cc."]),
        "tears" => (6, &["...c....c...", "...c....c..."]),
        "streams" => (5, &["..c......c..", ".cc......cc.", "cc........cc"]),
        "angry" => (2, &["..kk....kk..", "...kk..kk..."]),
        "sad" => (2, &["...kk..kk...", "..kk....kk.."]),
        "raised" => (2, &["........kkk."]),
        "cowboy" => (0, &["..nnnnnnnn..", "nnnnnnnnnnnn"]),
        "party" => (0, &["....PPPP....", "...PyPyPP..."]),
        "halo" => (0, &["...oooooo..."]),
        "horns" => (0, &["kk........kk", ".kk......kk."]),
        "mask" => (7, &["..wwwwwwww..", "..wlwlwlwl..", "..wwwwwwww..", "...wwwwww..."]),
        "thermo" => (8, &[".......kkkkk", ".......rrrwk"]),
        "bandage" => (1, &["...tttttttt.", "..tkttttkt.."]),
        "zzz" => (0, &[".......ccc..", ".........c..", "........c...", ".......ccc.."]),
        "steam" => (3, &["l..........l", "l..........l"]),
        "nose" => (6, &["......tttt..", "......tttttt"]),
        "heart" => (0, &["........rr.r", ".........rr."]),
        "sparks" => (1, &["y..........y", "...........y"]),
        "hand" => (7, &["....yyyy....", "...ykkky....", "....yyyy...."]),
        "glasses" => (4, &[".kkkkkkkkkk."]),
        "floppy_n" => (1, &[".nn......nn.", "nnn......nnn", "nnn......nnn", "nn........nn"]),
        "ears_s" => (0, &[".kkk....kkk.", "kssk....kssk"]),
        "ears_n" => (0, &[".kkk....kkk.", "knnk....knnk"]),
        "ears_k" => (0, &[".kkk....kkk.", "kkkk....kkkk"]),
        "ears_p" => (0, &[".kk......kk.", "kppk....kppk"]),
        "tall_w" => (0, &["..kk....kk..", ".kwwk..kwwk.", ".kwpk..kpwk.", ".kwwk..kwwk."]),
        "nose_p" => (6, &[".....pp....."]),
        "nose_k" => (6, &[".....kk....."]),
        "nose_big" => (6, &[".....kk.....", "....kkkk....", "....kkkk...."]),
        "muzzle_e" => (6, &["...eeeeee...", "..eekkkkee..", "...eeeeee..."]),
        "muzzle_t" => (6, &["...tttttt...", "...ttkktt...", "....tttt...."]),
        "muzzle_p" => (6, &["...pppppp...", "..pkppppkp..", "...pppppp..."]),
        "fox_cheeks" => (7, &["..wwwwwwww..", "...wwwwww..."]),
        "snout" => (6, &["...rrrrrr...", "...rkrrkr...", "...rrrrrr..."]),
        "cow_spots" => (1, &["..kk........", "............", "........kk..", "..........kk"]),
        "horns_y" => (0, &["yy........yy"]),
        "frogeyes" => (0, &[".kkk....kkk.", "kwwwk..kwwwk", "kwkwk..kwkwk"]),
        "stripes" => (1, &["....k..k....", "k..........k", "kk........kk"]),
        "pre_mane" => (2, &["...yyyyyy...", "..yyyyyyyy..", ".yyyyyyyyyy.", ".yyyyyyyyyy.", ".yyyyyyyyyy.", ".yyyyyyyyyy.", ".yyyyyyyyyy.", "..yyyyyyyy..", "...yyyyyy..."]),
        "monocle" => (3, &["......kkkk..", "......kyyk..", "......kyyk..", "......kkkk.."]),
        other => panic!("no extra called {other}"),
    }
}

/// One face: `(name, characters, head, body colour, eyes, mouth, extras)`.
type Spec = (&'static str, &'static str, &'static str, char, &'static str, &'static str, &'static [&'static str]);

const SPECS: &[Spec] = &[
    ("grinning", "😀", DISC, 'y', "dot", "grin", &[]),
    ("smiley", "😃", DISC, 'y', "wide", "grin", &[]),
    ("smile", "😄", DISC, 'y', "happy", "grin", &[]),
    ("grin", "😁", DISC, 'y', "happy", "teeth", &[]),
    ("laughing", "😆", DISC, 'y', "squint", "grin", &[]),
    ("sweat_smile", "😅", DISC, 'y', "dot", "grin", &["sweat"]),
    ("rofl", "🤣", DISC, 'y', "squint", "laugh", &["streams"]),
    ("joy", "😂", DISC, 'y', "happy", "laugh", &["streams"]),
    ("slight_smile", "🙂", DISC, 'y', "dot", "smile", &[]),
    ("wink", "😉", DISC, 'y', "wink", "smile", &[]),
    ("blush", "😊", DISC, 'y', "happy", "smile", &["blush"]),
    ("innocent", "😇", DISC, 'y', "dot", "smile", &["halo"]),
    ("smiling_hearts", "🥰", DISC, 'y', "happy", "smile", &["blush", "heart"]),
    ("heart_eyes", "😍", DISC, 'y', "hearts", "grin", &[]),
    ("star_struck", "🤩", DISC, 'y', "stars", "grin", &[]),
    ("kissing_heart", "😘", DISC, 'y', "wink", "kiss", &["heart"]),
    ("kissing", "😗", DISC, 'y', "dot", "kiss", &[]),
    ("kissing_smiling_eyes", "😙", DISC, 'y', "happy", "kiss", &[]),
    ("kissing_closed_eyes", "😚", DISC, 'y', "closed", "kiss", &["blush"]),
    ("relaxed", "☺", DISC, 'y', "dot", "smile", &["blush"]),
    ("yum", "😋", DISC, 'y', "happy", "tongue", &[]),
    ("stuck_out_tongue", "😛", DISC, 'y', "dot", "tonguefl", &[]),
    ("stuck_out_tongue_winking_eye", "😜", DISC, 'y', "wink", "tonguefl", &[]),
    ("zany", "🤪", DISC, 'y', "cross", "tonguefl", &[]),
    ("stuck_out_tongue_closed_eyes", "😝", DISC, 'y', "squint", "tonguefl", &[]),
    ("money_mouth", "🤑", DISC, 'y', "dollar", "grin", &[]),
    ("hugging", "🤗", DISC, 'y', "happy", "smile", &["blush", "sparks"]),
    ("hand_over_mouth", "🤭", DISC, 'y', "wide", "none", &["hand"]),
    ("shushing", "🤫", DISC, 'y', "dot", "none", &["hand"]),
    ("thinking", "🤔", DISC, 'y', "dot", "wavy", &["raised"]),
    ("zipper_mouth", "🤐", DISC, 'y', "dot", "zip", &[]),
    ("raised_eyebrow", "🤨", DISC, 'y', "dot", "flat", &["raised"]),
    ("neutral", "😐", DISC, 'y', "dot", "flat", &[]),
    ("expressionless", "😑", DISC, 'y', "closed", "flat", &[]),
    ("no_mouth", "😶", DISC, 'y', "dot", "none", &[]),
    ("smirk", "😏", DISC, 'y', "lids", "smirk", &[]),
    ("unamused", "😒", DISC, 'y', "lids", "frown", &["sad"]),
    ("roll_eyes", "🙄", DISC, 'y', "up", "flat", &[]),
    ("grimacing", "😬", DISC, 'y', "wide", "grimace", &[]),
    ("lying", "🤥", DISC, 'y', "dot", "flat", &["nose"]),
    ("relieved", "😌", DISC, 'y', "closed", "smile", &["blush"]),
    ("pensive", "😔", DISC, 'y', "closed", "frown", &["sad"]),
    ("sleepy", "😪", DISC, 'y', "closed", "small", &["tears"]),
    ("drooling", "🤤", DISC, 'y', "closed", "drool", &[]),
    ("sleeping", "😴", DISC, 'y', "closed", "small", &["zzz"]),
    ("mask", "😷", DISC, 'y', "dot", "none", &["mask"]),
    ("thermometer_face", "🤒", DISC, 'y', "lids", "flat", &["thermo"]),
    ("head_bandage", "🤕", DISC, 'y', "lids", "frown", &["bandage"]),
    ("nauseated", "🤢", DISC, 'g', "lids", "wavy", &[]),
    ("vomiting", "🤮", DISC, 'g', "closed", "vomit", &[]),
    ("sneezing", "🤧", DISC, 'y', "closed", "open", &["sweat"]),
    ("hot", "🥵", DISC, 'r', "lids", "tonguefl", &["sweat"]),
    ("cold", "🥶", DISC, 'c', "wide", "teeth", &["sweat"]),
    ("woozy", "🥴", DISC, 'y', "cross", "wavy", &[]),
    ("dizzy_face", "😵", DISC, 'y', "cross", "flat", &[]),
    ("exploding_head", "🤯", DISC, 'y', "wide", "open", &["sparks"]),
    ("cowboy", "🤠", DISC, 'y', "dot", "smile", &["cowboy"]),
    ("partying", "🥳", DISC, 'y', "happy", "grin", &["party"]),
    ("sunglasses", "😎", DISC, 'y', "shades", "smile", &[]),
    ("nerd", "🤓", DISC, 'y', "nerd", "teeth", &[]),
    ("monocle", "🧐", DISC, 'y', "dot", "flat", &["monocle", "raised"]),
    ("confused", "😕", DISC, 'y', "dot", "wavy", &[]),
    ("worried", "😟", DISC, 'y', "dot", "frown", &["sad"]),
    ("slight_frown", "🙁", DISC, 'y', "dot", "frown", &[]),
    ("frowning", "☹", DISC, 'y', "dot", "frown", &["sad"]),
    ("open_mouth", "😮", DISC, 'y', "wide", "small", &[]),
    ("hushed", "😯", DISC, 'y', "dot", "small", &["raised"]),
    ("astonished", "😲", DISC, 'y', "wide", "open", &[]),
    ("flushed", "😳", DISC, 'y', "wide", "flat", &["blush"]),
    ("pleading", "🥺", DISC, 'y', "wide", "frown", &["sad"]),
    ("frowning_open", "😦", DISC, 'y', "dot", "sadopen", &[]),
    ("anguished", "😧", DISC, 'y', "wide", "sadopen", &["sad"]),
    ("fearful", "😨", DISC, 'y', "wide", "sadopen", &["sweat"]),
    ("cold_sweat", "😰", DISC, 'y', "wide", "sadopen", &["sweat", "sad"]),
    ("sad_relieved", "😥", DISC, 'y', "dot", "frown", &["sweat", "sad"]),
    ("cry", "😢", DISC, 'y', "dot", "frown", &["tears", "sad"]),
    ("sob", "😭", DISC, 'y', "closed", "cry", &["streams"]),
    ("scream", "😱", DISC, 'y', "wide", "scream", &["sweat"]),
    ("confounded", "😖", DISC, 'y', "squint", "wavy", &[]),
    ("persevere", "😣", DISC, 'y', "squint", "frown", &[]),
    ("disappointed", "😞", DISC, 'y', "closed", "frown", &["sad"]),
    ("sweat", "😓", DISC, 'y', "dot", "frown", &["sweat"]),
    ("weary", "😩", DISC, 'y', "squint", "cry", &[]),
    ("tired_face", "😫", DISC, 'y', "squint", "teeth", &[]),
    ("yawning", "🥱", DISC, 'y', "closed", "open", &["tears"]),
    ("triumph", "😤", DISC, 'y', "lids", "frown", &["steam", "angry"]),
    ("pout", "😡", DISC, 'r', "dot", "frown", &["angry"]),
    ("angry", "😠", DISC, 'o', "dot", "frown", &["angry"]),
    ("cursing", "🤬", DISC, 'r', "cross", "teeth", &["angry"]),
    ("smiling_imp", "😈", DISC, 'P', "lids", "grin", &["horns"]),
    ("imp", "👿", DISC, 'P', "lids", "frown", &["horns", "angry"]),
    ("clown", "🤡", DISC, 'w', "dot", "grin", &["blush"]),
    ("ogre", "👹", DISC, 'R', "lids", "grimace", &["horns", "angry"]),
    ("goblin", "👺", DISC, 'r', "lids", "teeth", &["nose", "angry"]),
    ("alien", "👽", DISC, 'g', "wide", "small", &[]),
    ("robot", "🤖", DISC, 'l', "stars", "zip", &["glasses"]),
    ("smiley_cat", "😺", CAT, 'y', "wide", "grin", &[]),
    ("smile_cat", "😸", CAT, 'y', "happy", "grin", &[]),
    ("joy_cat", "😹", CAT, 'y', "happy", "laugh", &["streams"]),
    ("heart_eyes_cat", "😻", CAT, 'y', "hearts", "grin", &[]),
    ("smirk_cat", "😼", CAT, 'y', "lids", "smirk", &[]),
    ("kissing_cat", "😽", CAT, 'y', "closed", "kiss", &[]),
    ("scream_cat", "🙀", CAT, 'y', "wide", "scream", &[]),
    ("crying_cat", "😿", CAT, 'y', "dot", "frown", &["tears"]),
    ("pouting_cat", "😾", CAT, 'o', "dot", "frown", &["angry"]),
    ("dog_face", "🐶", DISC, 't', "dot", "none", &["floppy_n", "muzzle_e"]),
    ("cat_face", "🐱", CAT, 'y', "dot", "smile", &["nose_p"]),
    ("mouse_face", "🐭", DISC, 's', "dot", "smile", &["ears_s", "nose_p"]),
    ("hamster_face", "🐹", DISC, 'o', "dot", "smile", &["ears_n", "blush"]),
    ("rabbit_face", "🐰", DISC, 'w', "dot", "smile", &["tall_w", "nose_p"]),
    ("fox_face", "🦊", CAT, 'o', "dot", "none", &["fox_cheeks", "nose_k"]),
    ("bear_face", "🐻", DISC, 'n', "dot", "none", &["ears_n", "muzzle_t"]),
    ("panda_face", "🐼", DISC, 'w', "panda", "smile", &["ears_k", "nose_k"]),
    ("koala_face", "🐨", DISC, 's', "dot", "none", &["ears_s", "nose_big"]),
    ("tiger_face", "🐯", DISC, 'o', "dot", "smile", &["stripes", "ears_n"]),
    ("lion_face", "🦁", DISC, 'o', "dot", "smile", &["pre_mane", "nose_k"]),
    ("pig_face", "🐷", DISC, 'p', "dot", "none", &["ears_p", "snout"]),
    ("cow_face", "🐮", DISC, 'w', "dot", "none", &["cow_spots", "horns_y", "muzzle_p"]),
    ("frog_face", "🐸", DISC, 'g', "none", "smile", &["frogeyes"]),
    ("monkey_face", "🐵", MONKEY, 'n', "dot", "smile", &[]),
    ("see_no_evil", "🙈", MONKEY, 'n', "none", "smile", &["hand"]),
    ("hear_no_evil", "🙉", MONKEY, 'n', "dot", "smile", &[]),
    ("speak_no_evil", "🙊", MONKEY, 'n', "dot", "none", &["hand"]),
];

fn compose(spec: &Spec) -> Grid {
    let (_, _, head, body, e, m, extras) = spec;
    let mut g = parse(head);
    if *body != 'y' {
        recolor(&mut g, 'y', *body);
    }
    // A white face (clown) keeps its outline; a face whose body is dark would hide the eyes, but
    // none of ours are.
    for x in extras.iter().filter(|x| x.starts_with("pre_")) {
        let (top, rows) = extra(x);
        overlay(&mut g, top, rows);
    }
    for (name, kind) in [(*e, 0), (*m, 1)] {
        let (top, rows) = if kind == 0 { eyes(name) } else { mouth(name) };
        overlay(&mut g, top, rows);
    }
    for x in extras.iter().filter(|x| !x.starts_with("pre_")) {
        let (top, rows) = extra(x);
        overlay(&mut g, top, rows);
    }
    g
}

pub fn all() -> Vec<Emoji> {
    let mut out: Vec<Emoji> = SPECS.iter().map(|s| emoji(s.0, s.1, compose(s))).collect();
    // The upside-down face is the smiling face turned over.
    let smile = SPECS.iter().find(|s| s.0 == "slight_smile").unwrap();
    out.push(emoji("upside_down", "🙃", flip_vertical(&compose(smile))));
    out
}
