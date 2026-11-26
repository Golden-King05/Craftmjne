//! Hand-drawn emoji: nature, weather, food, things, symbols, hands. Twelve rows of twelve palette letters.

use super::{emoji, flip_horizontal, overlay, parse, recolor, rotate_cw, Emoji};

/// `(name, characters, art)`.
pub const ART: &[(&str, &str, &str)] = &[
    // ---- hearts, stars, sparks
    ("heart", "❤", "............/..kkk..kkk../.krrrkkrrrk./krrwrrrrrrrk/krwrrrrrrrrk/krrrrrrrrrrk/.krrrrrrrrk./..krrrrrrk../...krrrrk.../....krrk..../.....kk...../............"),
    ("star", "⭐🌟", ".....kk...../....kyyk..../....kyyk..../kkkkkyykkkkk/kyyyyyyyyyyk/.kyyyyyyyyk./..kyyyyyyk../..kyyyyyyk../.kyyykkyyyk./.kyyk..kyyk./.kk......kk./............"),
    ("sparkles", "✨", "....y......../...yyy....y./....y....yyy./.........y../..y...........yyy.../.yyyyy...y..../..yyy...yyy.../...y.....y..../............/............/............/............"),
    ("fire", "🔥", ".....k....../....kok...../....kork.k../...korrkkok./..korrooork./..korooyoork/.korooyyyork/.korooyyyork/.korrooyoork/..korrooork./...kkrrrkk../.....kkk...."),
    ("droplet", "💧", ".....kk...../.....kck..../....kcck..../...kcccck.../..kccwcccck./..kcwccccck./.kccwcccccck/.kccccccccck/.kccccccccck/..kccccccck./...kkcccckk./.....kkkk..."),
    ("sun", "☀🌞", ".....yy...../.y...yy...y./..y.yyyy.y../...yyyyyy.../..yyyyyyyy../yyyyyyyyyyyy/yyyyyyyyyyyy/..yyyyyyyy../...yyyyyy.../..y.yyyy.y../.y...yy...y./.....yy....."),
    ("cloud", "☁", "............/............/............/....kkk...../...kwwwkkk../..kwwwwwwwk./.kwwwwwwwwwk/kwwwwwwwwwwk/kllllllllllk/.kkkkkkkkkk./............/............"),
    ("rain_cloud", "🌧", "............/............/....kkk...../...kwwwkkk../..kwwwwwwwk./.kwwwwwwwwwk/kllllllllllk/.kkkkkkkkkk./..c...c...c./.c...c...c../..c...c...c./............"),
    ("snowflake", "❄", ".....cc...../.c...cc...c./..c..cc..c../...c.cc.c.../....cccc..../cccccccccccc/cccccccccccc/....cccc..../...c.cc.c.../..c..cc..c../.c...cc...c./.....cc....."),
    ("zap", "⚡", "......yy..../.....yyy..../....yyy...../...yyy....../..yyyyyyy.../.yyyyyyyy.../.....yyy..../....yyy...../....yy....../...yy......./...y......../..y........."),
    ("collision", "💥", "....o...o..../.o...oy..o../..oo.oyy.o.../...ooyyyyoo../.oooyyyyyooo/oyyyyyyyyyyo/.oooyyyyyooo/...ooyyyyoo../..oo.oyy.oo../.o...oy...o./....o...o...../............"),
    ("anger", "💢", "............/.rr......rr./rr.r....r.rr/r..rr..rr..r/....rrrr..../.....rr...../.....rr...../....rrrr..../r..rr..rr..r/rr.r....r.rr/.rr......rr./............"),
    ("zzz", "💤", "............/.cccccc...../.....c....../....c......./...cccccc.../.........ccc./..........c../.........c.../........ccc../............/............/............"),
    ("speech", "💬", "............/.kkkkkkkkkk./kwwwwwwwwwwk/kwkwkwkwkwwk/kwwwwwwwwwwk/kwkwkwkwwwwk/.kkkkkkkkkk./...kk......../..kk.........k.........../............"),
    ("thought", "💭", "..kkkkkkkk.../.kwwwwwwwwk../kwwwwwwwwwwk/kwwwwwwwwwwk/kwwwwwwwwwwk/.kwwwwwwwwk./..kkkkkkkk.../...k........../..kwk......../...k........../............"),
    ("question", "❓❔", "...kkkkkk.../..krrrrrrk../..krkkkkrrk./.....kkkrrk./.....krrrk../....krrrk.../....krrk..../....krrk..../............/....krrk..../....krrk..../.....kk....."),
    ("exclamation", "❗❕", "....kkkkk..../...krrrrrk.../...krrrrrk.../...krrrrrk.../....krrrk..../....krrrk..../.....krk...../.....krk...../.....kkk...../.....kkk...../....krrrk..../.....kkk....."),
    ("check", "✅✔", "............/..........gg/.........ggg/........ggg./gg.....ggg../ggg...ggg.../.ggg.ggg..../..ggggg...../...ggg....../....g......../............/............"),
    ("cross", "❌✖", "............/.rr......rr./.rrr....rrr./..rrr..rrr../...rrrrrr.../....rrrr..../....rrrr..../...rrrrrr.../..rrr..rrr../.rrr....rrr./.rr......rr./............"),
    ("plus", "➕", "............/.....kk...../.....kk...../.....kk...../.kkkkkkkkkk./.kkkkkkkkkk./.....kk...../.....kk...../.....kk...../............/............/............"),
    ("minus", "➖", "............/............/............/............/............/.kkkkkkkkkk./.kkkkkkkkkk./............/............/............/............/............"),
    ("warning", "⚠", ".....kk...../....kyyk..../....kyyk..../...kyykkyk.../...kyykkyk.../..kyyykkyyk../..kyyykkyyk../.kyyyyyyyyyk./.kyyyykkyyyk./kyyyyyyyyyyk/kkkkkkkkkkkk/............"),
    ("no_entry", "⛔🚫", "...kkkkkk.../.kkrrrrrrkk./.krrrrrrrrk./krrrrrrrrrrk/krwwwwwwwwrk/krwwwwwwwwrk/krrrrrrrrrrk/.krrrrrrrrk./.kkrrrrrrkk./...kkkkkk.../............/............"),
    ("recycle", "♻", "............/....gggg..../...gg..gg.../..ggg...g.../.gggggg.gg../...ggg..ggg./......gg.gg../.gg..gggggg./.gg...ggg.../..gg..gg..../...gggg...../............"),
    ("hundred", "💯", "............/..rr.rrr.rrr/.rrr.r.r.r.r/..rr.r.r.r.r/..rr.r.r.r.r/..rr.r.r.r.r/.rrrr.rrr.rrr/............/.rrrrrrrrrrr/............/.rrrrrrrrrrr/............"),
    ("magnifier", "🔍", "...kkkk...../..kcccck..../.kcwccccck../.kcwccccck../.kccccccck../..kcccck..../...kkkkkk.../.......kkk../........kkk./.........kkk./..........kk./............"),
    // ---- nature
    ("tree", "🌳", "....kkkk..../..kkgggkk.../.kgggGgggk../kgggggggGggk/kgGgggggggGk/kgggggGgggkk/.kgggggggk../..kkgGgkk.../....knnk..../....knnk..../....knnk..../...kknnkk...."),
    ("evergreen", "🌲", ".....kk...../....kggk..../...kgggGk.../....kgggk..../...kgggggk.../..kgGgggggk../...kggggGk.../..kgggggggk../.kgGggggggGk/kkkkkknkkkkk/.....knnk..../.....kkkk...."),
    ("cactus", "🌵", ".....kkk..../....kgggk.../.kk.kgGgk.kk/kggkkggkkggk/kgGgkggkgGgk/kgggkgGgkggk/.kkkkgggkkkk./....kgGgk..../....kgggk..../....kgGgk..../....kkkkk..../............"),
    ("sprout", "🌱", "............/............/..kk....kk../.kggk..kggk./kgggk..kgggk/kgGgkkkkgGgk/.kkkgkkgkkk../....kggk..../....kggk..../....kggk..../...kkkkkk.../............"),
    ("four_leaf_clover", "🍀", "..kk....kk../.kggk..kggk./kggggkkggggk/kgGgggggggGk/.kggggGgggk../.kkgggggggkk./kggggkkggggk/kggggk.kggggk/.kkkkgk.kkkk../.....gk....../.....g......./............"),
    ("leaves", "🍃", "............/........kkkk/......kkgggk/....kkgggGgk/..kkgggggGgk/.kgggggGggk./kggggGgggk../kgGggggkk.../kgggkkk...../.kkk........./............/............"),
    ("mushroom", "🍄", "....kkkk..../..kkrrrrkk../.krrwwrrrrk./krrwwwrrwwrk/krrrwwrrwwrk/krrrrrrrrrrk/kkkkkwwkkkkk/....kwwk..../....kwwk..../....kwwk..../....kkkk..../............"),
    ("rose", "🌹", "....kkkk..../...krrrrk.../..krrRRrrk../..krRrrRrk../..krrrrrrk../...kkrrkk.../.....kg....../..kk.kgk.kk../.kggkkgkggk../..kgggkgkk.../....kgg....../....kkk......."),
    ("sunflower", "🌻", "....kyyk..../.kk.kyyk.kk./kyykkyykkyyk/.kyykkkkyykk./kykkNNNNkkyk/yyykNNNNkyyy/kykkNNNNkkyk/.kyykkkkyyk../kyykkgkkyykk/.kk..kgk..kk./.....kgk...../.....kkk....."),
    ("tulip", "🌷", "..kk..kk..kk./.kppkkppkkppk/.kppppppppk../..kpppppppk.../...kkpppkk..../.....kgk....../.....kgk.kk..../..kk.kgkkgk.../.kggkkgkggk.../..kgggkggk..../....kkgk....../.....kk......."),
    ("blossom", "🌸🌼", "....kk..kk.../...kppkkppk../..kppppppppk./..kpppyypppk./...kppyyppk.../..kppppppppk./..kpppkkpppk./...kkk..kkk.../............/............/............/............"),
    // ---- food
    ("apple", "🍎", ".....kk...../....kGGk..../..kkkkkkkk../.krrrkkrrrk./krrwrrrrrrrk/krwrrrrrrrrk/krrrrrrrrrrk/krrrrrrrrrrk/.krrrrrrrrk./..krrrrrrk../...kk..kk.../............"),
    ("tangerine", "🍊", ".....kk...../....kGGk..../..kkkkkkkk../.koooookkook./kooowooooook/koowoooooook/kooooooooook/kooooooooook/.koooooooook./..kooooook.../...kk..kk.../............"),
    ("lemon", "🍋", "............/............/....kkkk..../..kkyyyykk../.kyyyyyyyyk./kyywyyyyyyyk/kyyyyyyyyyyk/.kyyyyyyyyk./..kkyyyykk../....kkkk..../............/............"),
    ("banana", "🍌", "............/............/........kk../.......kyyk./......kyyyk./.....kyyyyk./....kyyyyk../k..kyyyyk.../kkkyyyyk..../.kyyyyk...../..kkkk......./............"),
    ("watermelon", "🍉", "............/............/kkkkkkkkkkkk/krrrrrrrrrrk/krrkrrrrkrrk/.krrrrrrrrk./.krrrkrrrrk./..krrrrrrk../..kwwwwwwk../...kGGGGk.../....kkkk..../............"),
    ("grapes", "🍇", "......kk..../.....kGGk.../...kkkGkkk../..kPPkPPkPPk./..kPwPkPwPkP./...kPPkPPkP.../..kPPkPPkPPk./...kPwPkPwPk../....kPPkPPk.../.....kPPk...../......kk....."),
    ("strawberry", "🍓", "...kGGGGk..../..kkGkkGkk.../.krrrkkrrrk../krrwrrrrwrrk/krrrrwrrrrrk/.krrrrrrrrk./.krwrrrrwrk./..krrrrrrk../...krrrrk.../....krrk..../.....kk......"),
    ("cherries", "🍒", "......kk...../.....k.kk..../....k...k.k../...k.....kk../..kk.....k.../.krrk...krrk./krrwrk.krrwrk/krrrrk.krrrrk/krrrrk.krrrrk/.kkkk...kkkk./............"),
    ("peach", "🍑", ".....kk...../....kGGk..../..kkpppkk.../.kpppkkppk../kppwpppppppk/kpwpppppppk./kppppppppppk/.kppppppppk./..kppppppk../...kkppkk..../.....kk......"),
    ("pineapple", "🍍", "....g.g.g..../.....ggg...../....kgGgk..../...kkkkkkk.../..kyykyykyk../..kykyykyyk../..kyykyykyk../..kykyykyyk../...kyykyyk.../....kkkkkk.../............"),
    ("carrot", "🥕", "..g.g.g....../...ggg......./....kkk...../...koook..../...koook..../..kooook...../..koooook..../...kooook..../....koook...../.....kook...../......kk......"),
    ("corn", "🌽", ".....kk...../....kyyk..../...kyykyyk.../...kyykyyk.../...kyykyyk.../..kgykyykgk../..kgkyykygk../...kgkyykgk.../....kgygk..../.....kgk...../......k......."),
    ("tomato", "🍅", "....kGGk..../...kkGGkk.../..kkkkkkkk../.krrrrrrrrk./krrwrrrrrrrk/krwrrrrrrrrk/krrrrrrrrrrk/krrrrrrrrrrk/.krrrrrrrrk./..kkrrrrkk../....kkkk..../............"),
    ("eggplant", "🍆", "..........gg../.........kgGk./........kPPPk./.......kPPPPk./......kPPwPk../.....kPPPPPk../....kPPPPPk.../...kPPPPPk..../..kPPPPPk...../..kPPPPk....../...kkkk......./............"),
    ("avocado", "🥑", "....kkkk..../...kGGGGk.../..kGyyyyGk../..kGyyyyGk../.kGyyynnyyGk/.kGyyynnyyGk/.kGyyynnyyGk/..kGyyyyyGk../..kGGyyyGGk../...kGGGGGk.../....kkkkk..../............"),
    ("pizza", "🍕", "kkkkkkkkkkkk/kyyyyyyyyyyk/.kyryyyyryk./.kyyyyrryyk./..kyyyyyyk../..kyryyyrk.../...kyyyyk..../...kyyryk..../....kyyk...../....kyk....../.....k......./............"),
    ("hamburger", "🍔", "...kkkkkkk.../.knnnnnnnnk./knnwnnnwnnnk/kkkkkkkkkkkk/kgGgGgGgGgGk/krrrrrrrrrrk/kNNNNNNNNNNk/kkkkkkkkkkkk/knnnnnnnnnnk/.knnnnnnnnk./..kkkkkkkk../............"),
    ("fries", "🍟", "..y.y.y.y.y./..y.y.y.y.y./.ky.y.y.y.yk/.kyyyyyyyyyk/.krrrrrrrrrk/..krrrwwrrrk./..krrrwwrrk../..krrrrrrrk../...krrrrrk.../....kkkkk..../............/............"),
    ("hot_dog", "🌭", "............/............/........kkk./......kknnnk/....kkrnnnnk./..kkorrnnnk../.kooorrnnk.../kooooonnnk..../kkooonnk....../..kkkk......./............/............"),
    ("cake", "🍰", "............/............/....kkkk..../...kwwwwk.../..kwppwwwk../.kkkkkkkkkk./.kwwwwwwwwk./.krrkwwkrrk./.kwwwwwwwwk./.kkkkkkkkkk./............/............"),
    ("birthday", "🎂", ".....o......./.....y......./.....k......./..kkkkkkkk../.kwpwpwpwpk./.kkkkkkkkkk./.kppppppppk./.kpkpkpkpkk./.kppppppppk./.kkkkkkkkkk./.kwwwwwwwwk./.kkkkkkkkkk."),
    ("cookie", "🍪", "....kkkk..../..kknnnnkk../.knnNnnnnnk./knnnnnnNnnnk/knNnnnnnnnnk/knnnnNnnnNnk/knnnnnnnnnnk/knNnnnnNnnnk/.knnnnnnnnk./..kknnnnkk../....kkkk..../............"),
    ("doughnut", "🍩", "....kkkk..../..kkppppkk../.kppkkkkppk./kpwpkNNkpppk/kppkNNNNkppk/kppkNNNNkppk/kppkkNNkpwpk/.kppkkkkppk./..kkppppkk../....kkkk..../............/............"),
    ("ice_cream", "🍦", ".....kk...../....kwwk..../...kwwwwk.../..kwwlwwwk../...kkkkkk.../...knknnnk.../....knnnk..../....knnnk..../.....knk...../.....kk....../............"),
    ("coffee", "☕", "...s.s.s...../....s.s.s..../............/.kkkkkkkkk../.kwwwwwwwkkk./.knnnnnnnkwwk/.knnnnnnnkwwk/.knnnnnnnkwk./..knnnnnnkk../..kkkkkkkk.../............/............"),
    ("beer", "🍺", "..kwwwwwwk.../.kwwwwwwwwk../.kyyyyyyyykkk/.kyyyyyyyykwk/.kyyyyyyyykwk/.kyyyyyyyykwk/.kyyyyyyyykwk/.kyyyyyyyykkk/.kyyyyyyyyk../..kkkkkkkk.../............/............"),
    ("wine", "🍷", "..kkkkkkkk../.kwrrrrrrwk./.krrrrrrrrk./.krrrrrrrrk./..krrrrrrk../...kkrrkk.../.....kk....../.....kk....../.....kk....../...kkkkkkk.../............/............"),
    ("egg", "🥚", "....kkkk..../...kwwwwk.../..kwwwwwwk../.kwwwwwwwwk./.kwwwwwwwwk./kwwwwwwwwwwk/kwwwwwwwwwwk/.kwwwwwwwwk./..kkwwwwkk../....kkkk..../............/............"),
    ("bread", "🍞", "............/....kkkkk..../..kknnnnnkk../.knnnnnnnnnk./knnnnnnnnnnk/knnnnnnnnnnk/.kkeeeeeeekk/..keeeeeeeek../..keeeeeeeek../..kkkkkkkkkk../............/............"),
    ("cheese", "🧀", "............/............/...kkkkkkkkk./..kyyyyyyyyyk/.kyyoyyyyyyyk/kkkkkkkkkyyk/kyyyyoyyyyyk/kyyyyyyyoyyk/kyoyyyyyyykk/kkkkkkkkkkkk./............/............"),
    ("candy", "🍬", "............/.kk......kk./kppk.kk.kppk/kpppkkkkpppk/.kpkpwwpkpk./.kpkpppwkpk./kpppkkkkpppk/kppk.kk.kppk/.kk......kk./............/............/............"),
    ("lollipop", "🍭", "...kkkkkk.../..kpppppwk../.kppkkkkppk./.kpkwppkpwk./.kpkpppkppk./.kppkkkkppk./..kppppppk.../...kkppkk..../.....kk....../.....kk....../.....kk......"),
    ("chocolate", "🍫", "............/..kkkkkkkkkk/.kNNkNNkNNNk/.kNNkNNkNNNk/.kkkkkkkkkkk/.kNNkNNkNNNk/.kNNkNNkNNNk/.kkkkkkkkkkk/.kNNkNNkNNNk/..kkkkkkkkkk/............/............"),
    ("popcorn", "🍿", ".w.ww.w.ww../wwwwwwwwwww./.kwwwwwwwwk./kkrkwkrkwkrk/krrkkkrkrrrk/krrwkrrkrwrk/krrrkrrkrrrk/krrwkrrkrwrk/.krrkrrkrrk./.krrrrrrrrk./..kkkkkkkk../............"),
    ("milk", "🥛", "...kkkkkkkk../...kwwwwwwk../...kccccccck./...kwwwwwwwk./...kwwwwwwwk./...kwwwwwwwk./...kwwwwwwwk./....kwwwwwk../....kwwwwwk../....kkkkkkk../............/............"),
    ("meat", "🍖", "............/...kkkkkk.../..krrrrrrk../.krrrrrrrrk./.krrrRrrrrk./.krrrrrrRrk./..krrrrrrk.../kk.kkrrkk.../kwwk.kk....../.kwwk........./..kk.........."),
    ("poultry_leg", "🍗", "............/...kkkkkk.../..knnnnnnk../.knnnNnnnnk./.knnnnnnnNk./.knnNnnnnnk./..knnnnnnk.../...kknnkk..../.....kwk...../....kwwwk..../.....kkk....."),
    ("taco", "🌮", "............/............/...kkkkkkkk../..kyyyyyyyyk./.kGgGgGgGgGk/kgrgrggrgrgk/kyyyyyyyyyyk/kyyyyyyyyyyk/.kyyyyyyyyyk./..kkkkkkkkk.../............/............"),
    // ---- animals (full body or special)
    ("fish", "🐟", "............/............/.....kkk...../...kkcccckk.k/..kcccccwcckk/.kccccccckcck/.kcccccccccck/..kcccccccck.k/...kkcccckk.k./.....kkk...../............/............"),
    ("whale", "🐳", "....c..c...../.....cc....../............/..kkkkkkkk.../.kbbbbbbbbk../kbbbbwbbbbbk./kbbbbbbbbbbk./kbbwwwwwbbkk/.kkwwwwkkbbk/..kkkkkkkk.kbk/...........kk./............"),
    ("octopus", "🐙", "....kkkk..../..kkPPPPkk../.kPPPPPPPPk./kPPwkPPwkPPk/kPPkkPPkkPPk/kPPPPPPPPPPk/.kPPPPPPPPk./kPkPkPPkPkPk/kPkPkPPkPkPk/.k.k.kk.k.k./............/............"),
    ("bee", "🐝", "............/....kk.kk.../...kwwkwwk../..kwwwkwwwk./...kyykkyyk../..kykyyykyyk./..kyyyyyyyyk./..kykyyykykk./...kyykkyyk../....kkkkk..k./............/............"),
    ("lady_beetle", "🐞", "....k..k..../.....kk....../...kkkkkkk.../..krrrkrrrk../.krkrrkrrkrk./.krrrrkrrrrk./.krrkrkrkrrk./.krrrrkrrrrk./..krkrkrkrk../...krrkrrk.../....kkkkk..../............"),
    ("butterfly", "🦋", "k..........k./kk........kk/kPkk....kkPk/kPPPkkkkPPPk/.kPPPkkPPPk./.kPPbkkbPPk./..kbbbkbbbk../..kbbbkbbbk../...kbbkbbk.../....kkkkk..../............/............"),
    ("snake", "🐍", "...kkkk...../..kGggGk..../..kGkggGk.k../..kGggGkkrk../...kkGGk....../.....kGGk...../....kGggGk..../...kGggGk...../..kGggGk....../..kGGGk......./...kkk........"),
    ("turtle", "🐢", "............/....kkkk..../..kkGGGGkk../.kGgGGgGGgk./kGGgGGGGgGGk/kGgGGgGGGgGk/.kGGgGGgGGk./.kk.kkkkkk.k./.kg.k....k.gk/..kk......kk./............/............"),
    ("bat", "🦇", "............/k..k....k..k/kk.kk..kk.kk/kSkkSkkSkkSk/kSSSSSSSSSSk/kSSwSSSSwSSk/.kSSSSSSSSk./..kSSkkSSk.../...kkSSkk..../............/............/............"),
    ("snail", "🐌", "............/............/...kkkkk...../..knnNnnk..../.knnNnnNnk.../knnNnnnnnnk../knnnNnNnnnk../.knnnnnnnnk.k./.kkkkkkkkkkkk./kttttttttttk./.kkkkkkkkkk../............"),
    ("crab", "🦀", "kr.k....k.rk/krrk....krrk/.kk.kkkk.kk./..kkrrrrkk../.krrwrrwrrk./krrrkrrkrrrk/krrrrrrrrrrk/.kkrrrrrrkk./k.k.kkkk.k.k/.k.k....k.k./............/............"),
    ("dragon", "🐲", "k.kk....kk.k/kgkkgggkkgk./kggggGggggk./.kgwkgggwkgk/.kgkkgggkkgk/.kggggkkggk./.kggkrrkggk../..kgggggggk../...kkgggkk.../.....kkk....../............/............"),
    ("bone", "🦴", "....kk..kk.../...kwwkkwwk../..kwwwwwwwwk./...kkwwwwkk.../.....kwwk....../...kkwwwwkk.../..kwwwwwwwwk./...kwwkkwwk../....kk..kk.../............/............/............"),
    ("owl", "🦉", "k..........k/kn........nk/knnkkkkkknnk/knnwwknnwwnk/knwkkwnwkwnk/knnwwknnwwnk/knnnnyynnnnk/.knnnyyynnnk./.knnnnnnnnk./..kknnnnkk../...kyykyyk.../...kkk.kkk..."),
    ("duck", "🦆", "....kkkk..../...kyyyyk.../...kykyyk.../..oooyyyyk../...kkyyyyk.../.....kyyyyk../...kkyyyyyyk./..kyyyyyyyyk./..kyyyyyyyyk./...kkkkkkkk../............/............"),
    ("chicken", "🐔", "...rr.rr...../...kkkkkk.../..kwwwwwwk../..kwkwwkwwk./..kwwwoowwk./..kwwwooowk./...kwwwwwk.../..kwwwwwwwk../..kwwwwwwwk../...kkyykkyk.../....yy..yy..../............"),
    ("penguin", "🐧", "....kkkk..../...kSSSSk.../..kSSwwSSk../..kSwkwwkSk./..kSwwooSSk./.kSSwwwwwSSk/.kSwwwwwwwSk/.kSwwwwwwwSk/..kSwwwwwSk../..kkSSSSSkk../...oo...oo.../............"),
    ("bird", "🐦", "............/....kkkk..../...kbbbbk.../..kbbwkbbk../..kbbbbbbk../.oobbbbbbk../..kkbbbbbbk../....kbbbbbk.../....kkbbbbk.../......ooo.oo../............/............"),
    ("unicorn", "🦄", ".......y..../......yy..../.....yyk..../..kkkkwwk..../.kwwwwwwwk.../kwwkwwwwwwk../kwwwwwwwwwk../.kwwwwwppwk../..kwwwwpppk.../..kwwwwwwwk.../...kkkkkkkk.../............"),
    // ---- things
    ("key", "🔑", "...kkkk....../..kyyyyk...../.kyykkyyk..../.kyk..kyk..../.kyykkyyk..../..kyyyyyk..../...kkyyyyk.../.....kkyyyk../......kkyyyk./.......kkyyk./.........kk../............"),
    ("lock", "🔒", "...kkkkkk.../..ksllllsk../..ks....sk../..ks....sk../.kkkkkkkkkk./.kyyyyyyyyk./.kyyyykkyyk./.kyyyykkyyk./.kyyyyyyyyk./.kyyyyyyyyk./.kkkkkkkkkk./............"),
    ("bell", "🔔", ".....kk...../...kkyykk.../..kyyyyyyk../..kyyyyyyk../.kyyyyyyyyk./.kyyyyyyyyk./.kyyyyyyyyk./kyyyyyyyyyyk/kkkkkkkkkkkk/.....kyk...../......k......./............"),
    ("bomb", "💣", "........yy.o/.......kyk.../......kkk....y./....kkkkk.yo../..kkSSSSSkk.../.kSSwSSSSSk../kSSwSSSSSSSk./kSSSSSSSSSSk./kSSSSSSSSSSk./.kSSSSSSSSk../..kkSSSSkk.../....kkkk......"),
    ("gift", "🎁", "..kkk..kkk../.krrrkkrrrk./..kkkrrkkk../kkkkkkrkkkkk/kbbbbbrbbbbk/kbbbbbrbbbbk/krrrrrrrrrrk/kbbbbbrbbbbk/kbbbbbrbbbbk/kbbbbbrbbbbk/kkkkkkkkkkkk./............"),
    ("balloon", "🎈", "...kkkkk...../..krrrrrk..../.krwrrrrrk.../.krwrrrrrk.../.krrrrrrrk.../..krrrrrk..../...kkrrkk...../.....kk......./.....k........./......k......./.....k........."),
    ("tada", "🎉", "..y...p...../.k.kyk.....y./..kyyyk.o..../.kyyyyk.....c./kyyrryyk.g..../kyyryyyk...../kyryyyyk..p../kkyyyyk...../.kkkkk......./............/............/............"),
    ("trophy", "🏆", "kkkkkkkkkkk./kyyyyyyyyyyk/kyk.yyyy.kyk/.kk.kyyk.kk./....kyyk..../.....kyk...../.....kyk...../....kyyyk..../...kyyyyyk.../...kkkkkkk.../............/............"),
    ("medal", "🏅", ".kbbk..kbbk./..kbbkkbbk../...kbbbbk.../....kbbk..../...kkkkkk.../..kyyyyyyk../.kyyywwyyyk./.kyyywyyyyk./.kyyyyyyyyk./..kyyyyyyk../...kkkkkk.../............"),
    ("crown", "👑", "............/k..k.kk.k..k/ky.kykkyk.yk/kyykyyyykyyk/kyyyyyyyyyyk/kyyyrykyyryk/kyyyyyyyyyyk/kyyyyyyyyyyk/kkkkkkkkkkkk/............/............/............"),
    ("gem", "💎", "............/..kkkkkkkk../.kcwcccccck./kcwwccccccck/kkkkkkkkkkkk/.kccwccccck../..kccwcccck.../...kccwcck..../....kcckk...../.....kk......./............/............"),
    ("bulb", "💡", "....kkkk..../..kkyyyykk../.kyywyyyyyk./kyywyyyyyyyk/kyyyyyyyyyyk/.kyyyyyyyyk./..kyyyyyyk../...kkyykk..../....klllk..../....kllk...../.....kkk....."),
    ("book", "📖", "............/.kkkkkkkkkk./kwwwwkwwwwwk/kwkkwkwkkkwk/kwwwwkwwwwwk/kwkkkkwkkkwk/kwwwwkwwwwwk/kwkkwkwkkkwk/kwwwwkwwwwwk/.kkkkkkkkkk./............/............"),
    ("closed_book", "📕", "............/.kkkkkkkkk../krrrrrrrrrk./krkkkkkkkrk./krrrrrrrrrk./krkkkkkkrrk./krrrrrrrrrk./krrrrrrrrrk./krrrrrrrrrk./kwwwwwwwwwk./.kkkkkkkkk.../............"),
    ("pencil", "✏", "........kkk./.......kyyyk/......kyyykk/.....kyyykk./....kyyykk../...kyyykk.../..kyyykk..../.kttkk......./.kttk........./.kkk.........../............"),
    ("scissors", "✂", "............/.ss....ss.../..sk..ks..../...sk.ks...../....sksk....../.....ss......./....skks....../...kk..kk..../..kr....rk.../..kr....rk.../...kk..kk..../............"),
    ("hammer", "🔨", "........kkkk../.......ksssk./......ksssslk/.....kssssslk/....kknsssslk/...kknk.kkkk./..kknk......../.kknk........./kknk.........../knk............/kk............./............"),
    ("pick", "⛏", "..kkkkkkkk.../.ksssssssk../kssk....ksk../ksk......kk../kk..kn....../.....knk...../......knk..../.......knk.../........knk../.........knk./..........kk../............"),
    ("sword", "⚔🗡", ".........kk../........klsk./.......klsk../......klsk.../.....klsk..../.k..klsk...../kyk.klsk...../.kykklk....../..kyyk......../.kkykk......../kn.kk.k......./k............."),
    ("bow_and_arrow", "🏹", "...kk.......k./..knk.....klk/.knk.....klk./.knk....klk.../.knk...klk..../.knk..kkk...../.knkkklk....../.knk.kk........./..knk.k.........../...kk.k.........../............/............"),
    ("shield", "🛡", "............/.kkkkkkkkkk./.kbbbbkrrrk./.kbbbbkrrrk./.kbbbbkrrrk./.kkkkkkkkkk./.krrrkbbbbk./..krrkbbbk.../...krkbbk..../....kkkk...../............/............"),
    ("axe", "🪓", ".....kkkk...../....ksssk..../...ksssssk.../..kssslkk..../.ksssl.kn..../..kkkl..kn.../......kn..kn../.......kn..kn./........kn.kn./.........knk../..........k.../"),
    ("wrench", "🔧", "..kkk.kkk..../.ksssk.sk..../.ksk..kssk.../..kk..kssk.../.....ksssk.../....ksssk..../...ksssk...../..ksssk....../.kssskk....../.ksskk......./.kkk.........."),
    ("magnet", "🧲", "............/.kkkkkkkkk../krrrk.krrrk./krrrk.kbbbk./krrrk.kbbbk./krrrk.kbbbk./kwwwk.kwwwk./.kkk...kkk.../............/............/............/............"),
    ("coin", "🪙", "....kkkk..../..kkyyyykk../.kyyYYYYyyk./kyyYyyyyYyyk/kyYyyyyyyYyk/kyYyyyyyyYyk/kyYyyyyyyYyk/kyyYyyyyYyyk/.kyyYYYYyyk./..kkyyyykk../....kkkk..../............"),
    ("money_bag", "💰", "....kkkk..../...kyyyyk.../....kkkk..../...kGGGGk.../..kGGGGGGk../.kGGGyyGGGk./kGGGyGGGGGGk/kGGGGyyGGGGk/kGGGGGGyGGGk/.kGGGyyGGGk./..kkGGGGkk../....kkkk...."),
    ("mobile_phone", "📱", "...kkkkkk.../..kSSSSSSk../..kcccccck../..kcwcccck../..kccccccck./..kccccccck./..kccccccck./..kccccccck./..kSSSSSSk../..kSSkkSSk../...kkkkkk.../............"),
    ("laptop", "💻", "............/..kkkkkkkk../.kSSSSSSSSk./.kScccccccSk/.kSccccccccSk/.kScccccccSk./.kSSSSSSSSk./kkkkkkkkkkkk/kssssssssssk/kkkkkkkkkkkk/............/............"),
    ("camera", "📷", "............/...kkkk...../.kkkSSkkkkk./kSSSSSSSSSSk/kSSkkkkkSSSk/kSkcccckSSSk/kSkcwccckSSk/kSkcccckSSSk/kSSkkkkkSSSk/kSSSSSSSSSSk/.kkkkkkkkkk./............"),
    ("alarm_clock", "⏰", ".kk......kk./kRRkkkkkkRRk/.kkkwwwwkkk./..kwwwkwwwk../.kwwwwkwwwwk./.kwwwwkkkwwk./.kwwwwwwwwwk./.kwwwwwwwwwk./..kwwwwwwwk../..kkkwwwkkk../.kk.kkkkk.kk./............"),
    ("hourglass", "⌛", ".kkkkkkkkkk./.kyyyyyyyyk./..kyyyyyyk../...kyyyyk.../....kyyk..../.....kk...../.....kk...../....kyyk..../...kyyyyk.../..kyyyyyyk../.kyyyyyyyyk./.kkkkkkkkkk."),
    ("house", "🏠", ".....kk...../....krrk..../...krrrrk.../..krrrrrrk../.krrrrrrrrk./krrrrrrrrrrk/.kwwwwwwwwk./.kwwkbbkwwk./.kwwkbbkwwk./.kwwkbbkwwk./.kkkkkkkkkk./............"),
    ("car", "🚗", "............/............/....kkkkk..../...kcccccck.../..kccckccck../.krrrrrrrrrk./krrrrrrrrrrk/krrrrrrrrrrk/kkSSkkkkSSkk/.kSSk...kSSk./..kk.....kk../............"),
    ("bus", "🚌", "............/kkkkkkkkkkkk/kyyyyyyyyyyk/kcckcckcckck/kcckcckcckck/kyyyyyyyyyyk/kyyyyyyyyyyk/kkSSkkkkkSSk/.kSSk...kSSk./..kk.....kk../............/............"),
    ("rocket", "🚀", "........kkkk../.......kwwwwk./......kwwwcwk./.....kwwwwcwk./....kwwwwwwk../...krwwwwwk.../..krrkwwwk..../.krrrkkkk....../.kyrk.kok......./.kyk...kk......./..k.........../............"),
    ("ship", "🚢", ".....kk...../.....kwk...../....kwwwk..../.kkkkkkkkkkk./.kwwwcwcwcwk./.kwwwwwwwwwk./kbbbbbbbbbbk/.kbbbbbbbbbk./..kkbbbbkkk../............/............/............"),
    ("airplane", "✈", ".......kk...../.......kck..../......kcck..../.....kccck..../kk..kcccck.kk./kskkkccckksk/.kssssssssssk/.kkkkkcccckkk./......kcck..../.....kckk..../.....kk......./............"),
    ("bicycle", "🚲", "........kk.../.......kSSk../....kkkkkS.../...k.k.kk..../.kkk.k..k.kkk/kS..kkkkkk..Sk/kS..k.kk.k..Sk/kS.k..kk..k.Sk/.kkk.......kkk./............/............/............"),
    ("soccer_ball", "⚽", "....kkkk..../..kkwwwwkk../.kwwkkkkwwk./kwkkkSSkkkwk/kwkSSSSSSkwk/kkkSSSSSSkkk/kwkkSSSSkkwk/kwwkkkkkkwwk/.kwwwkkwwwk./..kkwwwwkk../....kkkk..../............"),
    ("basketball", "🏀", "....kkkk..../..kkoookk.k./.kooookooook/kooooookoook./koooookooook./kkkkkkkkkkkk/kooooookoook./koooookooook/.kooookoook../..kkoookk.../....kkkk..../............"),
    ("dice", "🎲", "............/.kkkkkkkkkk./.kwwwwwwwwk./.kwkwwwwkwk./.kwwwwwwwwk./.kwwwkkwwwk./.kwwwkkwwwk./.kwwwwwwwwk./.kwkwwwwkwk./.kwwwwwwwwk./.kkkkkkkkkk./............"),
    ("video_game", "🎮", "............/............/..kkkkkkkkk../.kSSSSSSSSSk./kSSkSSSrSSSk/kSkkkSSrrSSk/kSSkSSSrSSSk/kSSSSSSSSSSk/.kSSkkkkkkSSk./..kk......kk../............/............"),
    ("musical_note", "🎵", "......kkkkk../......kbbbk../......kk.bk../......k..bk../......k..bk../......k..bk../..kkk.k.kbk../.kbbbkk.kk.../.kbbbk......../..kkk........./............/............"),
    ("microphone", "🎤", "...kkkk...../..kSSSSk..../..kSllSk..../..kSllSk..../..kSSSSk..../...kkkk...../.....nk......./.....nk......./....nk........./...nk.........../..kk.........../............"),
    ("envelope", "✉", "............/............/kkkkkkkkkkkk/kwkwwwwwwkwk/kwwkwwwwkwwk/kwwwkwwkwwwk/kwwwwkkwwwwk/kwwwwwwwwwwk/kwwwwwwwwwwk/kkkkkkkkkkkk/............/............"),
    ("pushpin", "📌", "....kkkk..../...krrrrk.../...krwrrk.../...krrrrk.../....kkkk..../..kkkkkkkk../....kssk..../.....sk....../.....k......./............/............/............"),
    ("scroll", "📜", "..kkkkkkkk.../.kyyyyyyyyk../kyyk..kyyyk../kyyyyyyyyyk../.kyykkkkyyk../..kyyyyyyyk../..kyykkkkyk../..kyyyyyyyk../.kyyyyyyyyk../kyykkkkkkyk../kkyyyyyyyyk../.kkkkkkkkk..."),
    ("test_tube", "🧪", "...kkkkkk.../....kwwk..../....kwwk..../....kccck.../...kccccck../..kcccccck../..kcwcccck../..kccccccck./...kcccck.../....kkkk..../............/............"),
    ("potion", "🍶", "....kkkk..../....knnk..../....kwwk..../...kwwwwk.../..kwwwwwwk../.kpppppppk./kpppwppppppk/kppwpppppppk/kpppppppppk./.kpppppppk../..kkkkkkk.../............"),
    ("brick", "🧱", "............/kkkkkkkkkkkk/krrkrrrkrrrk/krrkrrrkrrrk/kkkkkkkkkkkk/krrrkrrrkrrk/krrrkrrrkrrk/kkkkkkkkkkkk/krrkrrrkrrrk/krrkrrrkrrrk/kkkkkkkkkkkk/............"),
    ("wood", "🪵", "............/..kkkkkkkk.../.knnnnnnnnk../knnkkkknnnnk./knkkNNkknnnk./knkNNNNknnnk./knkNNNNknnnk./knkkNNkknnnk./knnkkkknnnnk./.knnnnnnnnk../..kkkkkkkk.../............"),
    ("rock", "🪨", "............/............/....kkkkk..../..kkssssskk../.kslsssssssk./ksllsssssssk/kssssssSsssk/kssssssSSssk./.kssSssssSsk../..kkSSSSSkk.../....kkkkk..../............"),
    ("skull", "💀", "....kkkk..../..kkwwwwkk../.kwwwwwwwwk./kwwwwwwwwwwk/kwkkkwwkkkwk/kwkkkwwkkkwk/kwwwwkkwwwwk/.kwwwwwwwwk./..kwkwkwkwk../..kwkwkwkwk../...kkkkkkk.../............"),
    ("ghost", "👻", "....kkkk..../..kkwwwwkk../.kwwwwwwwwk./kwwwwwwwwwwk/kwwkkwwkkwwk/kwwkkwwkkwwk/kwwwwwwwwwwk/kwwwwkkwwwwk/kwwwwwwwwwwk/kwwkwwwwkwwk/.kk.kwwk.kk./............"),
    ("poop", "💩", ".....kk...../....knnk..../...knnnnk.../..knwknnwknk./.knnnkknnnnk./kknnnnnnnnkk/knnnnnnnnnnk/knnwnnnnwnnk/kNnnnnnnnnNk/.kNNNNNNNNNk./..kkkkkkkkk../............"),
    ("space_invader", "👾", "...k....k.../....k..k..../...kkkkkk.../..kkPkkPkk../.kkkkkkkkkk./.kkkkkkkkkk./.k.kkkkkk.k./.k.k....k.k./....kk..kk..../............/............/............"),
    // ---- hands
    ("thumbs_up", "👍", "......kk..../.....ktk..../....kttk..../....kttk..../.kkkkttkkkk./kttttttttttk/kttttttttttk/kttttttttttk/kttttttttttk/.kttttttttk./..kkkkkkkk../............"),
    ("thumbs_down", "👎", "............/..kkkkkkkk../.kttttttttk./kttttttttttk/kttttttttttk/kttttttttttk/kttttttttttk/.kkkkttkkkk./....kttk..../....kttk..../.....ktk...../......kk....."),
    ("victory", "✌", "...kk..kk.../..kttk.kttk../..kttk.kttk../..kttkkkttk../..kttttttk.../.kkttttttkk../kttttttttttk/kttttttttttk/.kttttttttk./..kttttttk.../...kkkkkk.../............"),
    ("fist", "✊👊", "............/..kkkkkkkk../.kttttttttk./kttkttktkttk/kttkttktkttk/kttttttttttk/kttttttttttk/.kttttttttk./..kttttttk.../...kkkkkk.../............/............"),
    ("wave", "👋", "..kk.kk.kk../.ktkktkktk../.ktkktkktk../.kttkttkttk./.kttttttttk./.kttttttttk./..kttttttk../..kttttttk../...kttttk.../....kkkk..../............/............"),
    ("clap", "👏", "............/.k..k..k..k./k.k.k..k.k.k/..kktk..ktkk./.kttk....kttk./kttk......kttk/kttk......kttk/.kttk....kttk./..kttk..kttk../...kkkkkkkk.../............/............"),
    ("pray", "🙏", "......kk..../.....kttk.../....kttttk../...kttkkttk./...kttkkttk./..kttkkkkttk./..kttkkkkttk./..kttttttttk./...kttttttk../....kkkkkk..../............/............"),
    ("muscle", "💪", "............/....kkkkk..../...kttttk..../..kttkttk..../..kttttttkkkk./..kttttttttttk/...kttttttttk./....kkttttkk../......kkkk..../............/............/............"),
    ("point_up", "☝", ".....kk...../....kttk..../....kttk..../....kttk..../....kttk..../..kkkttkkk.../.kttttttttk../.kttttttttk../.kttttttttk../..kttttttk.../...kkkkkk..../............"),
    ("raised_hand", "✋", "....kk.kk.kk./...ktkktkktk./...ktkktkktk./.kkktkktkktk./kttktttttttk/kttttttttttk/.kttttttttk./..kttttttk../...kttttk.../....kkkk..../............/............"),
    ("ok_hand", "👌", "............/...kk.kkk.../..ktkktttk../..kttkkttk.../..kttkktk..../.kkttttkk...../kttttttttkk../kttttttttttk./kttttttttttk./.kttttttttk../..kkkkkkkk.../............"),
];

fn ground_up() -> Vec<Emoji> {
    ART.iter().map(|(n, c, a)| emoji(n, c, parse(a))).collect()
}

pub fn all() -> Vec<Emoji> {
    let mut out = ground_up();
    let grid_of = |name: &str| out.iter().find(|e| e.name == name).map(|e| e.grid).unwrap();
    let mut extra: Vec<Emoji> = Vec::new();

    // The red heart in other colours.
    let heart = grid_of("heart");
    for (name, ch, to) in [("orange_heart", "🧡", 'o'), ("yellow_heart", "💛", 'y'), ("green_heart", "💚", 'g'), ("blue_heart", "💙", 'b'), ("purple_heart", "💜", 'P'), ("black_heart", "🖤", 'S'), ("white_heart", "🤍", 'w'), ("brown_heart", "🤎", 'n'), ("pink_heart", "💗", 'p')] {
        let mut g = heart;
        recolor(&mut g, 'r', to);
        extra.push(emoji(name, ch, g));
    }
    let mut broken = heart;
    overlay(&mut broken, 2, &["......k.....", ".....kk.....", "......k.....", ".....kk.....", "......k.....", ".....k......", "......k....."]);
    extra.push(emoji("broken_heart", "💔", broken));
    let mut sparkle_heart = heart;
    overlay(&mut sparkle_heart, 0, &["y..........y", "...........y"]);
    extra.push(emoji("sparkling_heart", "💖", sparkle_heart));

    // 🔫 is a water pistol in the current emoji standard (a toy, blue with an orange grip), pointing left.
    extra.push(emoji("water_pistol", "🔫", parse("............/............/..kkkkkkkk../.kbbbbbbbbk./kcwbbbbbbbbk/.kkkkkkbbbk./......kook../......kook../......kook../.......kk.../............/............")));

    // The two symbols the pirate and transgender flags are written with.
    extra.push(emoji("skull_and_crossbones", "\u{2620}", parse("............/...kkkkkk.../..kwwwwwwk../..kwkwwkwk../..kwkwwkwk.../...kwwwwk.../....kwwk..../..k.kkkk.k../...kk..kk.../...kk..kk.../..k.k..k.k../............")));
    extra.push(emoji("transgender_symbol", "\u{26A7}", parse("............/......pppp../.........pp./...cccc..pp./..c....c.p../..c....c..../..c....c..../...cccc...../....pp....../.pppppppp.../....pp....../............")));

    // Green apple, red and green pears from the apple.
    let apple = grid_of("apple");
    let mut green = apple;
    recolor(&mut green, 'r', 'g');
    extra.push(emoji("green_apple", "🍏", green));
    let mut pear = apple;
    recolor(&mut pear, 'r', 'y');
    extra.push(emoji("pear", "🍐", pear));

    // The pointing hand, turned to point every way.
    let up = grid_of("point_up");
    let right = rotate_cw(&up);
    extra.push(emoji("point_right", "👉", right));
    extra.push(emoji("point_down", "👇", rotate_cw(&right)));
    extra.push(emoji("point_left", "👈", flip_horizontal(&right)));
    extra.push(emoji("backhand_up", "👆", up));
    out.extend(extra);
    out
}

#[cfg(test)]
mod tests {
    use super::ART;

    #[test]
    fn no_drawing_loses_ink_when_fitted_to_the_grid() {
        let mut bad = Vec::new();
        for (name, _, art) in ART {
            let rows: Vec<&str> = art.split('/').collect();
            let cut = rows.iter().any(|r| crate::emoji::fit_row(r).1);
            if rows.len() > 12 || cut {
                bad.push(format!("{name}: {} rows", rows.len()));
            }
        }
        assert!(bad.is_empty(), "drawings that lose ink: {}", bad.join(", "));
    }
}
