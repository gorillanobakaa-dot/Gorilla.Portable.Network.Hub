//! The children's pages in the children's languages.
//!
//! The owner's decision (2026-09-24): English first, because many children
//! know a few words of it and "HELP" is understood almost everywhere; then
//! French, Swahili, Portuguese, Dari and Pashto, for the places this is for.
//! Dari and Pashto are written right to left, and the page flips with them.
//!
//! !!! THESE TRANSLATIONS ARE DRAFTS. They were written by an AI model
//! (Claude, 2026-09-24) and have NOT been checked by native speakers. A
//! mistranslated safety message is worse than none. Before any release that
//! puts them in front of a child, each language must be read by a native
//! speaker, ideally one who works with children, and corrected here. The
//! owner knows this; the release notes and the guide say it. !!!
//!
//! HOW IT WORKS. Every sentence a child sees is written in English in the page
//! code and passed through `t(lang, "...")`. The table below maps each English
//! sentence to its five translations; a sentence missing from the table shows
//! in English, and a test (`every_sentence_on_the_pages_is_in_the_table`)
//! fails if any `t(` call in page.rs has no row here, so nothing is left
//! untranslated by accident. `{n}`, `{name}` and `{size}` are filled in after
//! translation.
//!
//! The teacher's screens stay in English.

/// (code, name in its own language, right to left?, the HTML lang value)
pub const LANGS: &[(&str, &str, bool, &str)] = &[
    ("en", "English", false, "en"),
    ("fr", "Français", false, "fr"),
    ("sw", "Kiswahili", false, "sw"),
    ("pt", "Português", false, "pt"),
    ("prs", "دری", true, "fa-AF"),
    ("ps", "پښتو", true, "ps"),
];

pub fn known(code: &str) -> bool {
    LANGS.iter().any(|l| l.0 == code)
}

pub fn rtl(code: &str) -> bool {
    LANGS.iter().any(|l| l.0 == code && l.2)
}

/// `<html lang=".." dir="..">`
pub fn html_open(code: &str) -> String {
    let (lang, dir) = LANGS
        .iter()
        .find(|l| l.0 == code)
        .map(|l| (l.3, if l.2 { "rtl" } else { "ltr" }))
        .unwrap_or(("en", "ltr"));
    format!("<html lang=\"{lang}\" dir=\"{dir}\">")
}

fn column(code: &str) -> Option<usize> {
    match code {
        "fr" => Some(0),
        "sw" => Some(1),
        "pt" => Some(2),
        "prs" => Some(3),
        "ps" => Some(4),
        _ => None,
    }
}

/// One sentence in the child's language, or English.
pub fn t(lang: &str, en: &'static str) -> &'static str {
    let Some(c) = column(lang) else { return en };
    TABLE.iter().find(|(e, _)| *e == en).map(|(_, tr)| tr[c]).unwrap_or(en)
}

/// The same, with `{n}`, `{name}` and `{size}` filled in.
pub fn tf(lang: &str, en: &'static str, fill: &[(&str, &str)]) -> String {
    let mut s = t(lang, en).to_string();
    for (k, v) in fill {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// English, then French, Swahili, Portuguese, Dari, Pashto. DRAFTS: see the top.
#[rustfmt::skip]
pub const TABLE: &[(&str, [&str; 5])] = &[
    // ---- the name page
    ("Welcome to the class page", [
        "Bienvenue sur la page de la classe",
        "Karibu kwenye ukurasa wa darasa",
        "Bem-vindo à página da turma",
        "به صفحهٔ صنف خوش آمدید",
        "د ټولګي پاڼې ته ښه راغلاست",
    ]),
    ("First, type your name.", [
        "D'abord, écris ton nom.",
        "Kwanza, andika jina lako.",
        "Primeiro, escreve o teu nome.",
        "اول، نام خود را بنویسید.",
        "لومړی خپل نوم ولیکئ.",
    ]),
    ("Use the name your teacher calls you. When you send work, your teacher sees this name on it.", [
        "Utilise le nom que ton enseignant utilise pour toi. Quand tu envoies un travail, ton enseignant voit ce nom dessus.",
        "Tumia jina ambalo mwalimu wako anakuita. Ukituma kazi, mwalimu wako ataona jina hili juu yake.",
        "Usa o nome pelo qual o teu professor te chama. Quando enviares um trabalho, o teu professor vê este nome nele.",
        "نامی را بنویسید که معلم شما را به آن صدا می‌کند. وقتی کار خود را می‌فرستید، معلم این نام را روی آن می‌بیند.",
        "هغه نوم وکاروئ چې ښوونکی مو پرې غږوي. کله چې خپل کار لیږئ، ښوونکی به دا نوم پرې ووینی.",
    ]),
    ("THAT'S ME", [
        "C'EST MOI",
        "NI MIMI",
        "SOU EU",
        "این من هستم",
        "دا زه یم",
    ]),
    ("You only do this once. This page comes from your teacher's laptop through the class wifi. It works with no internet.", [
        "Tu ne le fais qu'une fois. Cette page vient de l'ordinateur de ton enseignant, par le wifi de la classe. Elle marche sans internet.",
        "Unafanya hivi mara moja tu. Ukurasa huu unatoka kwenye kompyuta ya mwalimu wako kupitia wifi ya darasa. Unafanya kazi bila intaneti.",
        "Só fazes isto uma vez. Esta página vem do computador do teu professor, pelo wifi da turma. Funciona sem internet.",
        "این کار را فقط یک بار انجام می‌دهید. این صفحه از کمپیوتر معلم شما از طریق وای‌فای صنف می‌آید و بدون انترنت کار می‌کند.",
        "دا کار یوازې یو ځل کوئ. دا پاڼه د ټولګي د وای‌فای له لارې ستاسو د ښوونکي له کمپیوټر څخه راځي او پرته له انټرنیټ کار کوي.",
    ]),
    ("Language", [
        "Langue",
        "Lugha",
        "Língua",
        "زبان",
        "ژبه",
    ]),
    // ---- the class page
    ("Class page", [
        "Page de la classe",
        "Ukurasa wa darasa",
        "Página da turma",
        "صفحهٔ صنف",
        "د ټولګي پاڼه",
    ]),
    ("You are <b>{name}</b>.", [
        "Tu es <b>{name}</b>.",
        "Wewe ni <b>{name}</b>.",
        "És <b>{name}</b>.",
        "شما <b>{name}</b> هستید.",
        "تاسو <b>{name}</b> یاست.",
    ]),
    ("Not you? Tap here to change it.", [
        "Ce n'est pas toi ? Touche ici pour le changer.",
        "Si wewe? Gusa hapa kubadilisha.",
        "Não és tu? Toca aqui para mudar.",
        "شما نیستید؟ برای تغییر اینجا را لمس کنید.",
        "تاسو نه یاست؟ د بدلولو لپاره دلته کېکاږئ.",
    ]),
    ("<b>Your work arrived.</b> It is on your teacher's laptop now, waiting for your teacher to accept it. You can send more, or close this page.", [
        "<b>Ton travail est arrivé.</b> Il est maintenant sur l'ordinateur de ton enseignant, qui doit l'accepter. Tu peux en envoyer d'autres, ou fermer cette page.",
        "<b>Kazi yako imefika.</b> Sasa iko kwenye kompyuta ya mwalimu wako, ikisubiri mwalimu aikubali. Unaweza kutuma zaidi, au kufunga ukurasa huu.",
        "<b>O teu trabalho chegou.</b> Está agora no computador do teu professor, à espera de ser aceite. Podes enviar mais, ou fechar esta página.",
        "<b>کار شما رسید.</b> اکنون در کمپیوتر معلم شماست و منتظر است که معلم آن را قبول کند. می‌توانید بیشتر بفرستید یا این صفحه را ببندید.",
        "<b>ستاسو کار ورسېد.</b> اوس د ښوونکي په کمپیوټر کې دی او انتظار باسي چې ښوونکی یې ومني. کولی شئ نور هم ولیږئ، یا دا پاڼه وتړئ.",
    ]),
    ("<b>Your note arrived.</b> Your teacher can read it on the laptop.", [
        "<b>Ton message est arrivé.</b> Ton enseignant peut le lire sur l'ordinateur.",
        "<b>Ujumbe wako umefika.</b> Mwalimu wako anaweza kuusoma kwenye kompyuta.",
        "<b>A tua mensagem chegou.</b> O teu professor pode lê-la no computador.",
        "<b>پیام شما رسید.</b> معلم شما می‌تواند آن را در کمپیوتر بخواند.",
        "<b>ستاسو پیغام ورسېد.</b> ښوونکی یې په کمپیوټر کې لوستلی شي.",
    ]),
    ("<b>Nothing was sent</b>, because no file was chosen. Under \"Send your work\", tap Choose files and pick your work first.", [
        "<b>Rien n'a été envoyé</b>, car aucun fichier n'était choisi. Sous « Envoie ton travail », touche Choisir des fichiers et choisis d'abord ton travail.",
        "<b>Hakuna kilichotumwa</b>, kwa sababu hakuna faili iliyochaguliwa. Chini ya \"Tuma kazi yako\", gusa Chagua faili na uchague kazi yako kwanza.",
        "<b>Nada foi enviado</b>, porque não escolheste nenhum ficheiro. Em \"Envia o teu trabalho\", toca em Escolher ficheiros e escolhe primeiro o teu trabalho.",
        "<b>چیزی فرستاده نشد</b>، چون هیچ فایلی انتخاب نشده بود. زیر «کار خود را بفرستید»، «انتخاب فایل» را لمس کنید و اول کار خود را انتخاب کنید.",
        "<b>هېڅ ونه لیږل شو</b>، ځکه چې هېڅ فایل نه و ټاکل شوی. د «خپل کار ولیږئ» لاندې «فایل وټاکئ» کېکاږئ او لومړی خپل کار وټاکئ.",
    ]),
    ("<b>That file is too big to send this way</b> (more than 1 GB). Ask your teacher what to do.", [
        "<b>Ce fichier est trop gros pour être envoyé ainsi</b> (plus de 1 Go). Demande à ton enseignant quoi faire.",
        "<b>Faili hilo ni kubwa mno kutumwa kwa njia hii</b> (zaidi ya GB 1). Muulize mwalimu wako ufanye nini.",
        "<b>Esse ficheiro é grande demais para enviar assim</b> (mais de 1 GB). Pergunta ao teu professor o que fazer.",
        "<b>این فایل برای فرستادن به این شکل خیلی بزرگ است</b> (بیشتر از ۱ گیگابایت). از معلم خود بپرسید چه کنید.",
        "<b>دا فایل د دې لارې د لیږلو لپاره ډېر لوی دی</b> (له ۱ ګیګابایټ څخه زیات). له ښوونکي وپوښتئ چې څه وکړئ.",
    ]),
    ("<b>Your teacher's laptop could not keep it.</b> Nothing is lost: your work is still on your phone. Tell your teacher.", [
        "<b>L'ordinateur de ton enseignant n'a pas pu le garder.</b> Rien n'est perdu : ton travail est toujours sur ton téléphone. Dis-le à ton enseignant.",
        "<b>Kompyuta ya mwalimu wako haikuweza kuihifadhi.</b> Hakuna kilichopotea: kazi yako bado iko kwenye simu yako. Mwambie mwalimu wako.",
        "<b>O computador do teu professor não conseguiu guardá-lo.</b> Nada se perdeu: o teu trabalho continua no teu telefone. Diz ao teu professor.",
        "<b>کمپیوتر معلم نتوانست آن را نگه دارد.</b> چیزی از بین نرفته است: کار شما هنوز در تیلفون شماست. به معلم خود بگویید.",
        "<b>د ښوونکي کمپیوټر ونشو کولی چې وساتي.</b> هېڅ نه دي ورک شوي: ستاسو کار لا هم ستاسو په ټیلیفون کې دی. خپل ښوونکي ته ووایئ.",
    ]),
    ("From your teacher", [
        "De ton enseignant",
        "Kutoka kwa mwalimu wako",
        "Do teu professor",
        "از طرف معلم شما",
        "ستاسو له ښوونکي",
    ]),
    ("Files from your teacher", [
        "Fichiers de ton enseignant",
        "Faili kutoka kwa mwalimu wako",
        "Ficheiros do teu professor",
        "فایل‌ها از طرف معلم",
        "له ښوونکي فایلونه",
    ]),
    ("<b>READ</b> or <b>PLAY</b>: look at it now. <b>GET IT</b>: keep a copy on this phone. Slide the list up and down to see every file; new files appear by themselves.", [
        "<b>LIRE</b> ou <b>JOUER</b> : regarde-le maintenant. <b>PRENDRE</b> : garde une copie sur ce téléphone. Fais glisser la liste vers le haut et le bas pour voir tous les fichiers ; les nouveaux apparaissent tout seuls.",
        "<b>SOMA</b> au <b>CHEZA</b>: iangalie sasa. <b>IPATE</b>: weka nakala kwenye simu hii. Telezesha orodha juu na chini kuona kila faili; faili mpya zinaonekana zenyewe.",
        "<b>LER</b> ou <b>TOCAR</b>: vê agora. <b>GUARDAR</b>: fica com uma cópia neste telefone. Desliza a lista para cima e para baixo para ver todos os ficheiros; os novos aparecem sozinhos.",
        "<b>خواندن</b> یا <b>پخش</b>: همین حالا ببینید. <b>گرفتن</b>: یک کاپی در این تیلفون نگه دارید. فهرست را بالا و پایین بکشید تا همهٔ فایل‌ها را ببینید؛ فایل‌های نو خودشان ظاهر می‌شوند.",
        "<b>لوستل</b> یا <b>غږول</b>: همدا اوس یې وګورئ. <b>اخیستل</b>: په دې ټیلیفون کې یوه کاپي وساتئ. لړلیک پورته او ښکته کش کړئ ترڅو ټول فایلونه ووینئ؛ نوي فایلونه پخپله راښکاره کېږي.",
    ]),
    ("Where do the files I GET go?", [
        "Où vont les fichiers que je PRENDS ?",
        "Faili ninazozipata zinaenda wapi?",
        "Para onde vão os ficheiros que GUARDO?",
        "فایل‌هایی که می‌گیرم کجا می‌روند؟",
        "هغه فایلونه چې اخلم چېرته ځي؟",
    ]),
    ("<b>Android:</b> open the <b>Files</b> app (on Samsung: <b>My Files</b>), then <b>Downloads</b>.<br><b>iPhone:</b> open the <b>Files</b> app, then <b>Downloads</b>.<br><b>Any phone:</b> your browser's menu (the three dots) has <b>Downloads</b> too.", [
        "<b>Android :</b> ouvre l'application <b>Fichiers</b> (sur Samsung : <b>Mes fichiers</b>), puis <b>Téléchargements</b>.<br><b>iPhone :</b> ouvre l'application <b>Fichiers</b>, puis <b>Téléchargements</b>.<br><b>Tout téléphone :</b> le menu de ton navigateur (les trois points) a aussi <b>Téléchargements</b>.",
        "<b>Android:</b> fungua programu ya <b>Faili</b> (kwenye Samsung: <b>Faili Zangu</b>), kisha <b>Vipakuliwa</b>.<br><b>iPhone:</b> fungua programu ya <b>Faili</b>, kisha <b>Vipakuliwa</b>.<br><b>Simu yoyote:</b> menyu ya kivinjari chako (vitone vitatu) pia ina <b>Vipakuliwa</b>.",
        "<b>Android:</b> abre a aplicação <b>Ficheiros</b> (na Samsung: <b>Os meus ficheiros</b>), depois <b>Transferências</b>.<br><b>iPhone:</b> abre a aplicação <b>Ficheiros</b>, depois <b>Transferências</b>.<br><b>Qualquer telefone:</b> o menu do teu navegador (os três pontos) também tem <b>Transferências</b>.",
        "<b>اندروید:</b> برنامهٔ <b>فایل‌ها</b> را باز کنید (در سامسونگ: <b>فایل‌های من</b>)، بعد <b>دانلودها</b>.<br><b>آیفون:</b> برنامهٔ <b>فایل‌ها</b> را باز کنید، بعد <b>دانلودها</b>.<br><b>هر تیلفون:</b> منوی مرورگر شما (سه نقطه) هم <b>دانلودها</b> دارد.",
        "<b>انډرایډ:</b> د <b>فایلونو</b> اپ پرانیزئ (په سامسونګ کې: <b>زما فایلونه</b>)، بیا <b>ډاونلوډونه</b>.<br><b>آیفون:</b> د <b>فایلونو</b> اپ پرانیزئ، بیا <b>ډاونلوډونه</b>.<br><b>هر ټیلیفون:</b> ستاسو د براوزر مینو (درې ټکي) هم <b>ډاونلوډونه</b> لري.",
    ]),
    ("Send your work to your teacher", [
        "Envoie ton travail à ton enseignant",
        "Tuma kazi yako kwa mwalimu wako",
        "Envia o teu trabalho ao teu professor",
        "کار خود را به معلم بفرستید",
        "خپل کار ښوونکي ته ولیږئ",
    ]),
    ("Tap <b>Choose files</b> below (some phones say <b>Browse</b>) and pick your work: a photo of your page, a document, a drawing.", [
        "Touche <b>Choisir des fichiers</b> ci-dessous (sur certains téléphones : <b>Parcourir</b>) et choisis ton travail : une photo de ta page, un document, un dessin.",
        "Gusa <b>Chagua faili</b> hapa chini (simu nyingine zinasema <b>Vinjari</b>) na uchague kazi yako: picha ya ukurasa wako, hati, mchoro.",
        "Toca em <b>Escolher ficheiros</b> em baixo (alguns telefones dizem <b>Procurar</b>) e escolhe o teu trabalho: uma foto da tua página, um documento, um desenho.",
        "<b>انتخاب فایل</b> را در پایین لمس کنید (در بعضی تیلفون‌ها <b>مرور</b> نوشته است) و کار خود را انتخاب کنید: عکس صفحهٔ خود، یک سند، یک نقاشی.",
        "لاندې <b>فایل وټاکئ</b> کېکاږئ (په ځینو ټیلیفونونو کې <b>لټون</b> لیکل شوي) او خپل کار وټاکئ: د خپلې پاڼې عکس، یو سند، یو انځور.",
    ]),
    ("Check the list. Picked the wrong one? Tap <b>REMOVE</b> next to it.", [
        "Vérifie la liste. Tu t'es trompé ? Touche <b>RETIRER</b> à côté.",
        "Angalia orodha. Umechagua isiyo sahihi? Gusa <b>ONDOA</b> kando yake.",
        "Verifica a lista. Escolheste o errado? Toca em <b>RETIRAR</b> ao lado.",
        "فهرست را ببینید. اشتباه انتخاب کردید؟ <b>حذف</b> را کنار آن لمس کنید.",
        "لړلیک وګورئ. غلط مو ټاکلی؟ د هغه تر څنګ <b>لرې کول</b> کېکاږئ.",
    ]),
    ("Tap <b>SEND IT TO YOUR TEACHER</b>. Keep this page open until the green box says it arrived.", [
        "Touche <b>ENVOYER À TON ENSEIGNANT</b>. Garde cette page ouverte jusqu'à ce que la case verte dise que c'est arrivé.",
        "Gusa <b>TUMA KWA MWALIMU WAKO</b>. Acha ukurasa huu wazi hadi kisanduku cha kijani kiseme imefika.",
        "Toca em <b>ENVIAR AO TEU PROFESSOR</b>. Mantém esta página aberta até a caixa verde dizer que chegou.",
        "<b>به معلم بفرست</b> را لمس کنید. این صفحه را باز نگه دارید تا کادر سبز بگوید رسید.",
        "<b>ښوونکي ته یې ولیږه</b> کېکاږئ. دا پاڼه پرانیستې وساتئ تر هغه چې شنه چوکاټ ووایي ورسېد.",
    ]),
    ("You can pick more than one, and pick again to add more.", [
        "Tu peux en choisir plusieurs, et choisir encore pour en ajouter.",
        "Unaweza kuchagua zaidi ya moja, na kuchagua tena kuongeza zaidi.",
        "Podes escolher mais do que um, e escolher outra vez para juntar mais.",
        "می‌توانید بیشتر از یکی انتخاب کنید، و دوباره انتخاب کنید تا بیشتر اضافه شود.",
        "تاسو له یوه زیات ټاکلی شئ، او بیا ټاکلو سره نور ور زیاتولی شئ.",
    ]),
    ("SEND IT TO YOUR TEACHER", [
        "ENVOYER À TON ENSEIGNANT",
        "TUMA KWA MWALIMU WAKO",
        "ENVIAR AO TEU PROFESSOR",
        "به معلم بفرست",
        "ښوونکي ته یې ولیږه",
    ]),
    ("START AGAIN", [
        "RECOMMENCER",
        "ANZA UPYA",
        "RECOMEÇAR",
        "از نو شروع کن",
        "بیا پیل کړه",
    ]),
    ("Tapping Choose files does nothing?", [
        "Toucher Choisir des fichiers ne fait rien ?",
        "Kugusa Chagua faili hakufanyi chochote?",
        "Tocar em Escolher ficheiros não faz nada?",
        "لمس کردن «انتخاب فایل» کاری نمی‌کند؟",
        "د «فایل وټاکئ» کېکاږل هېڅ نه کوي؟",
    ]),
    ("Then this page is open in the small wifi sign-in window some phones use. That window can show and download files, but it cannot send them. Open the page in your normal browser instead; you stay on the class wifi.", [
        "Alors cette page est ouverte dans la petite fenêtre de connexion wifi de certains téléphones. Cette fenêtre peut afficher et télécharger des fichiers, mais pas les envoyer. Ouvre plutôt la page dans ton navigateur habituel ; tu restes sur le wifi de la classe.",
        "Basi ukurasa huu uko wazi kwenye dirisha dogo la kuingia wifi ambalo simu nyingine hutumia. Dirisha hilo linaweza kuonyesha na kupakua faili, lakini haliwezi kuzituma. Fungua ukurasa kwenye kivinjari chako cha kawaida badala yake; unabaki kwenye wifi ya darasa.",
        "Então esta página está aberta na pequena janela de entrada no wifi que alguns telefones usam. Essa janela pode mostrar e transferir ficheiros, mas não os pode enviar. Abre a página no teu navegador normal; continuas no wifi da turma.",
        "پس این صفحه در پنجرهٔ کوچک ورود به وای‌فای باز است که بعضی تیلفون‌ها استفاده می‌کنند. آن پنجره می‌تواند فایل‌ها را نشان دهد و دانلود کند، اما نمی‌تواند بفرستد. به جای آن صفحه را در مرورگر عادی خود باز کنید؛ در وای‌فای صنف باقی می‌مانید.",
        "نو دا پاڼه د وای‌فای د ننوتلو په هغه کوچنۍ کړکۍ کې پرانیستې ده چې ځینې ټیلیفونونه یې کاروي. هغه کړکۍ فایلونه ښودلی او ډاونلوډ کولی شي، خو لیږلی یې نه شي. پر ځای یې پاڼه په خپل عادي براوزر کې پرانیزئ؛ د ټولګي په وای‌فای کې پاتې کېږئ.",
    ]),
    ("OPEN THIS IN MY BROWSER", [
        "OUVRIR DANS MON NAVIGATEUR",
        "FUNGUA KWENYE KIVINJARI CHANGU",
        "ABRIR NO MEU NAVEGADOR",
        "در مرورگر من باز کن",
        "په خپل براوزر کې یې پرانیزه",
    ]),
    ("or tap here", [
        "ou touche ici",
        "au gusa hapa",
        "ou toca aqui",
        "یا اینجا را لمس کنید",
        "یا دلته کېکاږئ",
    ]),
    ("If neither opens your browser: tap the three dots at the top of this window and choose \"Open in browser\" or \"Use this network as is\".", [
        "Si aucun n'ouvre ton navigateur : touche les trois points en haut de cette fenêtre et choisis « Ouvrir dans le navigateur » ou « Utiliser ce réseau tel quel ».",
        "Kama hakuna kinachofungua kivinjari chako: gusa vitone vitatu juu ya dirisha hili na uchague \"Fungua kwenye kivinjari\" au \"Tumia mtandao huu kama ulivyo\".",
        "Se nenhum abrir o teu navegador: toca nos três pontos no cimo desta janela e escolhe \"Abrir no navegador\" ou \"Usar esta rede tal como está\".",
        "اگر هیچ‌کدام مرورگر را باز نکرد: سه نقطه را در بالای این پنجره لمس کنید و «باز کردن در مرورگر» یا «استفاده از این شبکه همان‌طور که هست» را انتخاب کنید.",
        "که هېڅ یو هم براوزر پرانیست: د دې کړکۍ په سر کې درې ټکي کېکاږئ او «په براوزر کې پرانیستل» یا «دا شبکه همداسې وکاروئ» وټاکئ.",
    ]),
    ("Talk to your teacher", [
        "Parle à ton enseignant",
        "Ongea na mwalimu wako",
        "Fala com o teu professor",
        "با معلم خود صحبت کنید",
        "له خپل ښوونکي سره خبرې وکړئ",
    ]),
    ("Write a question or a message. Your teacher reads it on the laptop and can answer you here. New answers appear by themselves.", [
        "Écris une question ou un message. Ton enseignant le lit sur l'ordinateur et peut te répondre ici. Les nouvelles réponses apparaissent toutes seules.",
        "Andika swali au ujumbe. Mwalimu wako ataisoma kwenye kompyuta na anaweza kukujibu hapa. Majibu mapya yanaonekana yenyewe.",
        "Escreve uma pergunta ou uma mensagem. O teu professor lê-a no computador e pode responder-te aqui. As novas respostas aparecem sozinhas.",
        "یک سوال یا پیام بنویسید. معلم شما آن را در کمپیوتر می‌خواند و می‌تواند همین‌جا جواب بدهد. جواب‌های نو خودشان ظاهر می‌شوند.",
        "یوه پوښتنه یا پیغام ولیکئ. ښوونکی یې په کمپیوټر کې لولي او دلته درته ځواب درکولی شي. نوي ځوابونه پخپله راښکاره کېږي.",
    ]),
    ("Type your message here", [
        "Écris ton message ici",
        "Andika ujumbe wako hapa",
        "Escreve a tua mensagem aqui",
        "پیام خود را اینجا بنویسید",
        "خپل پیغام دلته ولیکئ",
    ]),
    ("SEND", [
        "ENVOYER",
        "TUMA",
        "ENVIAR",
        "بفرست",
        "ولیږه",
    ]),
    ("If something is wrong and you do not want to say it in front of others, you can talk privately to a trusted adult. Nobody else sees it.", [
        "Si quelque chose ne va pas et que tu ne veux pas le dire devant les autres, tu peux parler en privé à un adulte de confiance. Personne d'autre ne le voit.",
        "Kama kuna jambo baya na hutaki kulisema mbele ya wengine, unaweza kuongea kwa siri na mtu mzima unayemwamini. Hakuna mwingine atakayeona.",
        "Se alguma coisa está mal e não queres dizê-lo à frente dos outros, podes falar em privado com um adulto de confiança. Mais ninguém vê.",
        "اگر چیزی درست نیست و نمی‌خواهید آن را پیش دیگران بگویید، می‌توانید به صورت خصوصی با یک بزرگسال مورد اعتماد صحبت کنید. هیچ‌کس دیگر آن را نمی‌بیند.",
        "که یو څه سم نه وي او نه غواړئ چې د نورو په وړاندې یې ووایئ، کولی شئ په پټه له یو باوري لوی سړي سره خبرې وکړئ. بل هېڅوک یې نه ویني.",
    ]),
    ("HELP", [
        "AIDE",
        "MSAADA",
        "AJUDA",
        "کمک",
        "مرسته",
    ]),
    ("This page comes from your teacher's laptop through the class wifi, and works with no internet. If it stops working, check that your phone is still joined to the class wifi, then reload the page.", [
        "Cette page vient de l'ordinateur de ton enseignant par le wifi de la classe, et marche sans internet. Si elle ne marche plus, vérifie que ton téléphone est toujours sur le wifi de la classe, puis recharge la page.",
        "Ukurasa huu unatoka kwenye kompyuta ya mwalimu wako kupitia wifi ya darasa, na unafanya kazi bila intaneti. Ukiacha kufanya kazi, hakikisha simu yako bado imeunganishwa na wifi ya darasa, kisha pakia upya ukurasa.",
        "Esta página vem do computador do teu professor pelo wifi da turma, e funciona sem internet. Se deixar de funcionar, verifica se o teu telefone continua ligado ao wifi da turma, e depois recarrega a página.",
        "این صفحه از کمپیوتر معلم شما از طریق وای‌فای صنف می‌آید و بدون انترنت کار می‌کند. اگر کار نکرد، ببینید که تیلفون شما هنوز به وای‌فای صنف وصل است، بعد صفحه را دوباره بارگذاری کنید.",
        "دا پاڼه د ټولګي د وای‌فای له لارې ستاسو د ښوونکي له کمپیوټر څخه راځي، او پرته له انټرنیټ کار کوي. که کار یې بند کړ، وګورئ چې ټیلیفون مو لا هم د ټولګي له وای‌فای سره وصل دی، بیا پاڼه بیا راپورته کړئ.",
    ]),
    // ---- messages the page's scripts show
    ("Choose your work first: tap the Choose files button above and pick a file.", [
        "Choisis d'abord ton travail : touche le bouton Choisir des fichiers au-dessus et choisis un fichier.",
        "Chagua kazi yako kwanza: gusa kitufe cha Chagua faili hapo juu na uchague faili.",
        "Escolhe primeiro o teu trabalho: toca no botão Escolher ficheiros acima e escolhe um ficheiro.",
        "اول کار خود را انتخاب کنید: دکمهٔ «انتخاب فایل» را در بالا لمس کنید و یک فایل انتخاب کنید.",
        "لومړی خپل کار وټاکئ: پورته د «فایل وټاکئ» تڼۍ کېکاږئ او یو فایل وټاکئ.",
    ]),
    ("SENDING... KEEP THIS PAGE OPEN", [
        "ENVOI EN COURS... GARDE CETTE PAGE OUVERTE",
        "INATUMWA... ACHA UKURASA HUU WAZI",
        "A ENVIAR... MANTÉM ESTA PÁGINA ABERTA",
        "در حال فرستادن... این صفحه را باز نگه دارید",
        "لیږل کېږي... دا پاڼه پرانیستې وساتئ",
    ]),
    ("SENDING... {n}% - KEEP THIS PAGE OPEN", [
        "ENVOI... {n} % - GARDE CETTE PAGE OUVERTE",
        "INATUMWA... {n}% - ACHA UKURASA HUU WAZI",
        "A ENVIAR... {n}% - MANTÉM ESTA PÁGINA ABERTA",
        "در حال فرستادن... {n}٪ - این صفحه را باز نگه دارید",
        "لیږل کېږي... {n}٪ - دا پاڼه پرانیستې وساتئ",
    ]),
    ("It did not arrive. Check that this phone is still joined to the class wifi, then tap SEND IT TO YOUR TEACHER again.", [
        "Ce n'est pas arrivé. Vérifie que ce téléphone est toujours sur le wifi de la classe, puis touche encore ENVOYER À TON ENSEIGNANT.",
        "Haijafika. Hakikisha simu hii bado imeunganishwa na wifi ya darasa, kisha gusa TUMA KWA MWALIMU WAKO tena.",
        "Não chegou. Verifica se este telefone continua ligado ao wifi da turma, e toca outra vez em ENVIAR AO TEU PROFESSOR.",
        "نرسید. ببینید که این تیلفون هنوز به وای‌فای صنف وصل است، بعد دوباره «به معلم بفرست» را لمس کنید.",
        "ونه رسېد. وګورئ چې دا ټیلیفون لا هم د ټولګي له وای‌فای سره وصل دی، بیا بیا «ښوونکي ته یې ولیږه» کېکاږئ.",
    ]),
    ("REMOVE", [
        "RETIRER",
        "ONDOA",
        "RETIRAR",
        "حذف",
        "لرې کول",
    ]),
    ("1 file will be sent.", [
        "1 fichier sera envoyé.",
        "Faili 1 itatumwa.",
        "Vai ser enviado 1 ficheiro.",
        "۱ فایل فرستاده می‌شود.",
        "۱ فایل به ولیږل شي.",
    ]),
    ("{n} files will be sent.", [
        "{n} fichiers seront envoyés.",
        "Faili {n} zitatumwa.",
        "Vão ser enviados {n} ficheiros.",
        "{n} فایل فرستاده می‌شود.",
        "{n} فایلونه به ولیږل شي.",
    ]),
    // ---- conversations
    ("Nothing here yet. Only you and the trusted adult can see this.", [
        "Rien pour l'instant. Seuls toi et l'adulte de confiance pouvez voir ceci.",
        "Hakuna kitu bado. Ni wewe tu na mtu mzima unayemwamini mnaoweza kuona hiki.",
        "Ainda não há nada. Só tu e o adulto de confiança podem ver isto.",
        "هنوز چیزی نیست. فقط شما و بزرگسال مورد اعتماد این را می‌بینید.",
        "لا تر اوسه هېڅ نشته. یوازې تاسو او باوري لوی سړی دا لیدلی شئ.",
    ]),
    ("No messages yet. Write to your teacher below.", [
        "Pas encore de messages. Écris à ton enseignant ci-dessous.",
        "Hakuna ujumbe bado. Mwandikie mwalimu wako hapa chini.",
        "Ainda não há mensagens. Escreve ao teu professor em baixo.",
        "هنوز پیامی نیست. در پایین به معلم خود بنویسید.",
        "لا تر اوسه پیغام نشته. لاندې خپل ښوونکي ته ولیکئ.",
    ]),
    ("Trusted adult", [
        "Adulte de confiance",
        "Mtu mzima unayemwamini",
        "Adulto de confiança",
        "بزرگسال مورد اعتماد",
        "باوري لوی سړی",
    ]),
    ("Teacher", [
        "Enseignant",
        "Mwalimu",
        "Professor",
        "معلم",
        "ښوونکی",
    ]),
    ("You", [
        "Toi",
        "Wewe",
        "Tu",
        "شما",
        "تاسو",
    ]),
    ("read", [
        "lu",
        "imesomwa",
        "lido",
        "خوانده شد",
        "ولوستل شو",
    ]),
    ("seen by your teacher", [
        "vu par ton enseignant",
        "imeonwa na mwalimu wako",
        "visto pelo teu professor",
        "معلم آن را دید",
        "ښوونکي ولید",
    ]),
    ("<b>A trusted adult would like to talk to you.</b> Is that all right? Only they will see your answer.", [
        "<b>Un adulte de confiance aimerait te parler.</b> Est-ce que c'est d'accord ? Lui seul verra ta réponse.",
        "<b>Mtu mzima unayemwamini angependa kuongea nawe.</b> Je, ni sawa? Ni yeye tu atakayeona jibu lako.",
        "<b>Um adulto de confiança gostava de falar contigo.</b> Pode ser? Só ele vai ver a tua resposta.",
        "<b>یک بزرگسال مورد اعتماد می‌خواهد با شما صحبت کند.</b> آیا اشکالی ندارد؟ فقط او جواب شما را می‌بیند.",
        "<b>یو باوري لوی سړی غواړي له تاسو سره خبرې وکړي.</b> ایا سمه ده؟ یوازې هغه به ستاسو ځواب ووینی.",
    ]),
    ("YES", ["OUI", "NDIYO", "SIM", "بله", "هو"]),
    ("LATER", ["PLUS TARD", "BAADAYE", "MAIS TARDE", "بعداً", "وروسته"]),
    ("NO", ["NON", "HAPANA", "NÃO", "نخیر", "نه"]),
    ("Nothing was sent: write something first.", [
        "Rien n'a été envoyé : écris d'abord quelque chose.",
        "Hakuna kilichotumwa: andika kitu kwanza.",
        "Nada foi enviado: escreve primeiro alguma coisa.",
        "چیزی فرستاده نشد: اول چیزی بنویسید.",
        "هېڅ ونه لیږل شو: لومړی یو څه ولیکئ.",
    ]),
    ("Need help? One tap is enough. Only your teacher sees it.", [
        "Besoin d'aide ? Un seul toucher suffit. Seul ton professeur le voit.",
        "Unahitaji msaada? Mguso mmoja unatosha. Ni mwalimu wako tu anayeona.",
        "Precisas de ajuda? Um toque chega. Só o teu professor vê.",
        "کمک لازم دارید؟ یک لمس کافی است. فقط معلم شما آن را می‌بیند.",
        "مرستې ته اړتیا لرئ؟ یو کېکاږل بس دی. یوازې ستاسو ښوونکی یې ویني.",
    ]),
    ("Sent. Your teacher will find a safe moment to talk to you.", [
        "Envoyé. Ton professeur trouvera un moment sûr pour te parler.",
        "Imetumwa. Mwalimu wako atapata wakati salama wa kuongea nawe.",
        "Enviado. O teu professor vai encontrar um momento seguro para falar contigo.",
        "فرستاده شد. معلم شما یک وقت امن برای صحبت با شما پیدا می‌کند.",
        "ولیږل شو. ستاسو ښوونکی به له تاسو سره د خبرو لپاره یو خوندي وخت پیدا کړي.",
    ]),
    ("Sent. A trusted adult will find a safe moment to talk to you.", [
        "Envoyé. Un adulte de confiance trouvera un moment sûr pour te parler.",
        "Imetumwa. Mtu mzima unayemwamini atapata wakati salama wa kuongea nawe.",
        "Enviado. Um adulto de confiança vai encontrar um momento seguro para falar contigo.",
        "فرستاده شد. یک بزرگسال مورد اعتماد یک وقت امن برای صحبت با شما پیدا می‌کند.",
        "ولیږل شو. یو باوري لوی سړی به له تاسو سره د خبرو لپاره یو خوندي وخت پیدا کړي.",
    ]),
    ("That was a lot of messages. Wait one minute, then send again.", [
        "Ça fait beaucoup de messages. Attends une minute, puis envoie encore.",
        "Hiyo ni jumbe nyingi. Subiri dakika moja, kisha tuma tena.",
        "Foram muitas mensagens. Espera um minuto e depois envia outra vez.",
        "پیام‌های زیادی بود. یک دقیقه صبر کنید، بعد دوباره بفرستید.",
        "ډېر پیغامونه وو. یوه دقیقه انتظار وکړئ، بیا یې بیا ولیږئ.",
    ]),
    ("It did not arrive. Check that this phone is still on the class wifi, then send again.", [
        "Ce n'est pas arrivé. Vérifie que ce téléphone est toujours sur le wifi de la classe, puis envoie encore.",
        "Haijafika. Hakikisha simu hii bado iko kwenye wifi ya darasa, kisha tuma tena.",
        "Não chegou. Verifica se este telefone continua no wifi da turma, e envia outra vez.",
        "نرسید. ببینید که این تیلفون هنوز به وای‌فای صنف وصل است، بعد دوباره بفرستید.",
        "ونه رسېد. وګورئ چې دا ټیلیفون لا هم د ټولګي په وای‌فای کې دی، بیا یې بیا ولیږئ.",
    ]),
    ("I NEED TO TALK TO SOMEONE (one tap)", [
        "J'AI BESOIN DE PARLER À QUELQU'UN (un toucher)",
        "NAHITAJI KUONGEA NA MTU (mguso mmoja)",
        "PRECISO DE FALAR COM ALGUÉM (um toque)",
        "باید با کسی صحبت کنم (یک لمس)",
        "زه اړتیا لرم له چا سره خبرې وکړم (یو کېکاږل)",
    ]),
    ("I would like to talk to you. Is that all right?", [
        "J'aimerais te parler. Est-ce que c'est d'accord ?",
        "Ningependa kuongea nawe. Je, ni sawa?",
        "Gostava de falar contigo. Pode ser?",
        "می‌خواهم با شما صحبت کنم. آیا اشکالی ندارد؟",
        "غواړم له تاسو سره خبرې وکړم. ایا سمه ده؟",
    ]),
    ("Yes, I want to talk.", [
        "Oui, je veux parler.",
        "Ndiyo, nataka kuongea.",
        "Sim, quero falar.",
        "بله، می‌خواهم صحبت کنم.",
        "هو، غواړم خبرې وکړم.",
    ]),
    ("Later, not now.", [
        "Plus tard, pas maintenant.",
        "Baadaye, si sasa.",
        "Mais tarde, agora não.",
        "بعداً، حالا نه.",
        "وروسته، اوس نه.",
    ]),
    ("No, thank you.", [
        "Non, merci.",
        "Hapana, asante.",
        "Não, obrigado.",
        "نخیر، تشکر.",
        "نه، مننه.",
    ]),
    // ---- the private help page
    ("HIDE THIS (back to the files)", [
        "CACHER (retour aux fichiers)",
        "FICHA HII (rudi kwenye faili)",
        "ESCONDER (voltar aos ficheiros)",
        "این را پنهان کن (برگشت به فایل‌ها)",
        "دا پټ کړه (فایلونو ته بېرته)",
    ]),
    ("Talk privately", [
        "Parler en privé",
        "Ongea kwa siri",
        "Falar em privado",
        "صحبت خصوصی",
        "په پټه خبرې",
    ]),
    ("Only you and a trusted adult see this. Your teacher's screen does not show what you write. Nothing is kept on this phone: when you press HIDE, it is gone from here.", [
        "Seuls toi et un adulte de confiance voyez ceci. L'écran de ton enseignant ne montre pas ce que tu écris. Rien n'est gardé sur ce téléphone : quand tu touches CACHER, tout disparaît d'ici.",
        "Ni wewe tu na mtu mzima unayemwamini mnaoona hiki. Skrini ya mwalimu wako haionyeshi unachoandika. Hakuna kinachohifadhiwa kwenye simu hii: ukibonyeza FICHA, kinatoweka hapa.",
        "Só tu e um adulto de confiança veem isto. O ecrã do teu professor não mostra o que escreves. Nada fica guardado neste telefone: quando tocas em ESCONDER, desaparece daqui.",
        "فقط شما و یک بزرگسال مورد اعتماد این را می‌بینید. صفحهٔ معلم شما آنچه را می‌نویسید نشان نمی‌دهد. چیزی در این تیلفون نگه داشته نمی‌شود: وقتی «پنهان کن» را لمس کنید، از اینجا می‌رود.",
        "یوازې تاسو او یو باوري لوی سړی دا ګورئ. د ښوونکي پرده هغه څه نه ښیي چې تاسو یې لیکئ. په دې ټیلیفون کې هېڅ نه ساتل کېږي: کله چې «پټ کړه» کېکاږئ، له دې ځایه ځي.",
    ]),
    ("I NEED TO TALK TO SOMEONE", [
        "J'AI BESOIN DE PARLER À QUELQU'UN",
        "NAHITAJI KUONGEA NA MTU",
        "PRECISO DE FALAR COM ALGUÉM",
        "باید با کسی صحبت کنم",
        "زه اړتیا لرم له چا سره خبرې وکړم",
    ]),
    ("One tap is enough. You do not have to write anything. The trusted adult will find a safe moment to talk to you.", [
        "Un seul toucher suffit. Tu n'as rien à écrire. L'adulte de confiance trouvera un moment sûr pour te parler.",
        "Mguso mmoja unatosha. Huhitaji kuandika chochote. Mtu mzima unayemwamini atapata wakati salama wa kuongea nawe.",
        "Um toque chega. Não precisas de escrever nada. O adulto de confiança vai encontrar um momento seguro para falar contigo.",
        "یک لمس کافی است. لازم نیست چیزی بنویسید. بزرگسال مورد اعتماد یک وقت امن برای صحبت با شما پیدا می‌کند.",
        "یو کېکاږل بس دی. اړتیا نشته چې څه ولیکئ. باوري لوی سړی به له تاسو سره د خبرو لپاره یو خوندي وخت پیدا کړي.",
    ]),
    ("Or write here, if you want to", [
        "Ou écris ici, si tu veux",
        "Au andika hapa, ukitaka",
        "Ou escreve aqui, se quiseres",
        "یا اگر می‌خواهید اینجا بنویسید",
        "یا که غواړئ دلته ولیکئ",
    ]),
    ("SEND PRIVATELY", [
        "ENVOYER EN PRIVÉ",
        "TUMA KWA SIRI",
        "ENVIAR EM PRIVADO",
        "به صورت خصوصی بفرست",
        "په پټه یې ولیږه",
    ]),
    // ---- the list of files
    ("READ", ["LIRE", "SOMA", "LER", "خواندن", "لوستل"]),
    ("PLAY", ["JOUER", "CHEZA", "TOCAR", "پخش", "غږول"]),
    ("GET IT", ["PRENDRE", "IPATE", "GUARDAR", "گرفتن", "اخیستل"]),
    ("GET EVERYTHING ({n} files, {size})", [
        "TOUT PRENDRE ({n} fichiers, {size})",
        "PATA KILA KITU (faili {n}, {size})",
        "GUARDAR TUDO ({n} ficheiros, {size})",
        "گرفتن همه ({n} فایل، {size})",
        "ټول واخله ({n} فایلونه، {size})",
    ]),
    ("All the files in one download. A computer opens it like a folder; on a phone, open it from the Files app. Keep the phone or computer on until it has finished.", [
        "Tous les fichiers en un seul téléchargement. Un ordinateur l'ouvre comme un dossier ; sur un téléphone, ouvre-le depuis l'application Fichiers. Garde le téléphone ou l'ordinateur allumé jusqu'à la fin.",
        "Faili zote katika upakuaji mmoja. Kompyuta inaifungua kama folda; kwenye simu, ifungue kutoka programu ya Faili. Acha simu au kompyuta ikiwa imewashwa hadi imalize.",
        "Todos os ficheiros numa só transferência. Um computador abre-a como uma pasta; num telefone, abre-a na aplicação Ficheiros. Mantém o telefone ou o computador ligado até acabar.",
        "همهٔ فایل‌ها در یک دانلود. کمپیوتر آن را مثل یک پوشه باز می‌کند؛ در تیلفون، آن را از برنامهٔ فایل‌ها باز کنید. تیلفون یا کمپیوتر را تا آخر روشن نگه دارید.",
        "ټول فایلونه په یوه ډاونلوډ کې. کمپیوټر یې د یوه فولډر په څېر پرانیزي؛ په ټیلیفون کې یې د فایلونو له اپ څخه پرانیزئ. ټیلیفون یا کمپیوټر تر پایه روښانه وساتئ.",
    ]),
    ("Your teacher is not sharing any files yet. Just wait: this list checks again every 10 seconds by itself.", [
        "Ton enseignant ne partage encore aucun fichier. Attends : cette liste se remet à jour toute seule toutes les 10 secondes.",
        "Mwalimu wako bado hajashiriki faili yoyote. Subiri tu: orodha hii inaangalia tena kila sekunde 10 yenyewe.",
        "O teu professor ainda não está a partilhar ficheiros. Espera: esta lista volta a verificar sozinha a cada 10 segundos.",
        "معلم شما هنوز هیچ فایلی شریک نکرده است. فقط صبر کنید: این فهرست هر ۱۰ ثانیه خودش دوباره بررسی می‌کند.",
        "ښوونکي مو لا تر اوسه هېڅ فایل نه دی شریک کړی. یوازې انتظار وکړئ: دا لړلیک په هرو ۱۰ ثانیو کې پخپله بیا ګوري.",
    ]),
    ("<b>The folder cannot be reached right now.</b> If the files live on a USB drive, it may have been unplugged. Tell your teacher.", [
        "<b>Le dossier est inaccessible pour le moment.</b> Si les fichiers sont sur une clé USB, elle a peut-être été retirée. Dis-le à ton enseignant.",
        "<b>Folda haiwezi kufikiwa sasa hivi.</b> Kama faili ziko kwenye USB, huenda imechomolewa. Mwambie mwalimu wako.",
        "<b>Não é possível chegar à pasta agora.</b> Se os ficheiros estão numa pen USB, talvez tenha sido retirada. Diz ao teu professor.",
        "<b>فعلاً به پوشه دسترسی نیست.</b> اگر فایل‌ها روی فلش USB هستند، شاید کشیده شده باشد. به معلم خود بگویید.",
        "<b>اوس فولډر ته لاسرسی نشته.</b> که فایلونه په USB کې وي، کېدای شي ایستل شوی وي. خپل ښوونکي ته ووایئ.",
    ]),
    ("<b>{n} more files are being handed out than fit on this page.</b><br>The GET EVERYTHING button at the top has all of them.", [
        "<b>{n} fichiers de plus sont partagés que ce qui tient sur cette page.</b><br>Le bouton TOUT PRENDRE en haut les contient tous.",
        "<b>Faili {n} zaidi zinashirikiwa kuliko zinazotosha kwenye ukurasa huu.</b><br>Kitufe cha PATA KILA KITU juu kina zote.",
        "<b>Estão a ser partilhados mais {n} ficheiros do que cabem nesta página.</b><br>O botão GUARDAR TUDO no cimo tem-nos todos.",
        "<b>{n} فایل دیگر هم شریک شده که در این صفحه جا نمی‌شود.</b><br>دکمهٔ «گرفتن همه» در بالا همهٔ آن‌ها را دارد.",
        "<b>{n} نور فایلونه شریک شوي چې په دې پاڼه کې نه ځایېږي.</b><br>په سر کې د «ټول واخله» تڼۍ ټول لري.",
    ]),
    // ---- a paused phone, and the viewer
    ("Paused", ["En pause", "Imesimamishwa", "Em pausa", "متوقف شده", "درول شوی"]),
    ("Your teacher has paused this device.", [
        "Ton enseignant a mis cet appareil en pause.",
        "Mwalimu wako amesimamisha kifaa hiki.",
        "O teu professor pôs este aparelho em pausa.",
        "معلم شما این دستگاه را متوقف کرده است.",
        "ښوونکي مو دا وسیله درولې ده.",
    ]),
    ("You cannot get the class files or hand anything in right now. Nothing you send will arrive.", [
        "Tu ne peux pas prendre les fichiers de la classe ni rendre quoi que ce soit pour le moment. Rien de ce que tu envoies n'arrivera.",
        "Huwezi kupata faili za darasa wala kukabidhi chochote sasa hivi. Hakuna utakachotuma kitakachofika.",
        "Agora não podes guardar os ficheiros da turma nem entregar nada. Nada do que enviares vai chegar.",
        "فعلاً نمی‌توانید فایل‌های صنف را بگیرید یا چیزی تحویل دهید. هر چیزی بفرستید نمی‌رسد.",
        "اوس نه شئ کولی د ټولګي فایلونه واخلئ یا څه وسپارئ. هر څه چې ولیږئ، نه رسېږي.",
    ]),
    ("Speak to your teacher. When they let you back in, this page comes back by itself. You do not need to do anything.", [
        "Parle à ton enseignant. Quand il te laissera revenir, cette page reviendra toute seule. Tu n'as rien à faire.",
        "Ongea na mwalimu wako. Akikuruhusu kurudi, ukurasa huu utarudi wenyewe. Huhitaji kufanya chochote.",
        "Fala com o teu professor. Quando te deixar voltar, esta página volta sozinha. Não precisas de fazer nada.",
        "با معلم خود صحبت کنید. وقتی دوباره اجازه داد، این صفحه خودش برمی‌گردد. لازم نیست کاری کنید.",
        "له خپل ښوونکي سره خبرې وکړئ. کله چې بېرته اجازه درکړي، دا پاڼه پخپله بېرته راځي. اړتیا نشته چې څه وکړئ.",
    ]),
    ("BACK TO THE FILES", [
        "RETOUR AUX FICHIERS",
        "RUDI KWENYE FAILI",
        "VOLTAR AOS FICHEIROS",
        "برگشت به فایل‌ها",
        "فایلونو ته بېرته",
    ]),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row has all five translations, and no English sentence is in
    /// the table twice (the second would never be found).
    #[test]
    fn every_row_is_complete_and_unique() {
        for (i, (en, tr)) in TABLE.iter().enumerate() {
            for (c, s) in tr.iter().enumerate() {
                assert!(!s.trim().is_empty(), "row {i} ({en}) is missing language {c}");
                // A placeholder in the English must survive translation.
                for ph in ["{n}", "{name}", "{size}"] {
                    if en.contains(ph) {
                        assert!(s.contains(ph), "row {i} ({en}) lost {ph} in language {c}: {s}");
                    }
                }
            }
            assert!(TABLE.iter().filter(|(e, _)| e == en).count() == 1, "{en} is in the table twice");
        }
    }

    /// Every `t(lang, "...")` and `tf(lang, "...", ...)` on the children's
    /// pages has a row here, so nothing is left in English by accident.
    #[test]
    fn every_sentence_on_the_pages_is_in_the_table() {
        let src = include_str!("page.rs");
        let mut missing = Vec::new();
        let mut found = 0;
        for marker in ["t(l, \"", "tf(l, \""] {
            let mut rest = src;
            while let Some(at) = rest.find(marker) {
                rest = &rest[at + marker.len()..];
                // The string literal, with \" inside it.
                let mut s = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => match chars.next() {
                            Some('"') => s.push('"'),
                            Some('n') => s.push('\n'),
                            Some(o) => { s.push('\\'); s.push(o); }
                            None => break,
                        },
                        '"' => break,
                        c => s.push(c),
                    }
                }
                found += 1;
                if !TABLE.iter().any(|(e, _)| *e == s) {
                    missing.push(s);
                }
            }
        }
        assert!(found > 60, "only {found} sentences found: the scan is not reading page.rs");
        assert!(missing.is_empty(), "not in the table: {missing:#?}");
    }

    #[test]
    fn dari_and_pashto_flip_the_page() {
        assert!(html_open("prs").contains("dir=\"rtl\"") && html_open("ps").contains("dir=\"rtl\""));
        assert!(html_open("fr").contains("dir=\"ltr\"") && html_open("xx").contains("lang=\"en\""));
        assert_eq!(t("fr", "SEND"), "ENVOYER");
        assert_eq!(t("en", "SEND"), "SEND");
        assert_eq!(tf("sw", "{n} files will be sent.", &[("n", "3")]), "Faili 3 zitatumwa.");
    }
}
