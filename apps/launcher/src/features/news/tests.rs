use super::feed::{Fetched, Post, Trouble, embedded, format_date, image_format, merge, parse};
use super::state::{News, NewsScroll, StackedRow};

fn post(id: &str, date: &str) -> Post {
    Post {
        id: id.into(),
        kind: id.split('/').next().unwrap_or_default().into(),
        title: id.into(),
        date: date.into(),
        summary: String::new(),
        image: None,
        url: None,
        body: String::new(),
    }
}

#[test]
fn embeds_the_news_in_full() {
    // Unsere Nachrichten reisen mit dem Launcher.
    let posts = embedded();
    let news = posts
        .iter()
        .find(|p| p.title == "Willkommen im ERC Launcher")
        .expect("die Willkommensnachricht fehlt");
    assert_eq!(news.kind, "news");
    assert!(
        news.body.contains("## Wenn etwas nicht laeuft") || news.body.contains("## Wenn etwas nicht läuft")
    );
    assert!(posts.windows(2).all(|w| w[0].date >= w[1].date));
}

/// Eine fremde Versionsnotiz ist zweimal durchgerutscht (13.11. die 1.9.19–1.9.42,
/// 14.11. die 1.9.43–1.9.49): sie sind mit "1.9" groesser als "1.0" und kamen deshalb
/// an jedem Zahlenvergleich vorbei. Das Kennzeichen ist jetzt der Merker in der
/// Kopfzeile, nicht die Nummer. Fremde Notizen (etwa Swoftys 1.10.x) duerfen nach einem
/// Merge in `docs/releases/` liegen; eingebettet wird nur, was ihn traegt (build.rs) —
/// hier faellt auf, wenn doch eine fremde Notiz im Programm landet.
#[test]
fn no_foreign_release_note_reaches_the_program() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/releases");
    let eingebettet: Vec<String> = embedded()
        .iter()
        .filter(|p| p.release())
        .map(|p| p.version().to_owned())
        .collect();
    let mut fremde = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for path in entries.flatten().map(|e| e.path()) {
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let kopf: Vec<&str> = text.lines().skip(1).take_while(|l| l.trim() != "---").collect();
            if kopf.iter().any(|l| l.trim() == "product: erc") {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let version = name.strip_suffix(".md").unwrap_or(name);
            if eingebettet.iter().any(|v| v == version) {
                fremde.push(name.to_owned());
            }
        }
    }
    assert!(
        fremde.is_empty(),
        "fremde Versionsnotizen wurden eingebettet: {fremde:?}"
    );
}

#[test]
fn the_running_version_has_its_note() {
    // 1.1.0 ging ohne eigene Notiz hinaus, weil sie erst nach dem Bau geschrieben war.
    let fassung = env!("CARGO_PKG_VERSION");
    let posts = embedded();
    assert!(
        posts.iter().any(|p| p.release() && p.version() == fassung),
        "{fassung} hat keine Versionsnotiz im Programm"
    );
}

#[test]
fn merges_by_id_and_sorts_by_date_then_version() {
    let mut base = vec![
        post("release/1.2.9", "2026-10-21"),
        post("release/1.2.10", "2026-10-21"),
        post("news/hello", "2026-10-21"),
    ];
    base[2].body = "kept".into();
    let mut newer = vec![
        post("release/1.2.9", "2026-10-21"),
        post("news/hello", "2026-10-21"),
    ];
    newer[0].summary = "fresh".into();
    let merged = merge(base, newer);
    let ids: Vec<&str> = merged.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["news/hello", "release/1.2.10", "release/1.2.9"]);
    assert_eq!(merged[2].summary, "fresh");
    assert_eq!(merged[0].body, "kept");
}

#[test]
fn reads_new_and_old_feeds() {
    let posts = parse(r#"{"posts":[{"id":"news/a","title":"A","extra":1}],"version":2}"#).unwrap();
    assert_eq!(posts[0].title, "A");
    assert!(posts[0].body.is_empty());
    let posts = parse(r###"{"posts":[{"id":"news/b","kind":"news","title":"B","body":"## Hi\n\n- one"}]}"###)
        .unwrap();
    assert_eq!(posts[0].body, "## Hi\n\n- one");
    assert!(parse("<html>").is_none());
}

#[test]
fn says_why_the_latest_news_is_missing() {
    let mut news = News::load(std::path::Path::new("/nonexistent"));
    assert_eq!(
        news.subtitle(),
        "Offline · showing the news that came with this version"
    );
    news.trouble = Some(Trouble::Failed);
    assert_eq!(
        news.subtitle(),
        "Couldn't load the latest news · showing the news that came with this version"
    );
    news.saved = true;
    news.trouble = Some(Trouble::Offline);
    assert_eq!(news.subtitle(), "Offline · showing the news saved last time");
    news.live = true;
    assert_eq!(news.subtitle(), "");
}

#[test]
fn formats_dates() {
    assert_eq!(format_date("2026-10-22"), "22 October 2026");
    assert_eq!(format_date("soon"), "soon");
}

#[test]
fn knows_image_formats() {
    assert!(image_format(b"\x89PNG\r\n").is_some());
    assert!(image_format(b"<svg").is_none());
}

#[test]
fn stacked_sync_tracks_banner_visibility() {
    let news = News::load(std::path::Path::new("/nonexistent"));
    let mut scroll = NewsScroll::default();

    scroll.sync(&news, false, true, true);
    let initial = scroll.stacked_rows.len();
    assert!(matches!(scroll.stacked_rows.first(), Some(StackedRow::Banner)));
    assert_eq!(scroll.stacked.item_count(), initial);

    scroll.sync(&news, false, true, false);
    assert_eq!(scroll.stacked_rows.len(), initial - 1);
    assert!(!matches!(scroll.stacked_rows.first(), Some(StackedRow::Banner)));
    assert_eq!(scroll.stacked.item_count(), initial - 1);

    scroll.sync(&news, false, true, true);
    assert_eq!(scroll.stacked_rows.len(), initial);
    assert!(matches!(scroll.stacked_rows.first(), Some(StackedRow::Banner)));
    assert_eq!(scroll.stacked.item_count(), initial);
}

#[test]
fn a_news_check_bumps_the_revision_only_when_the_posts_change() {
    let mut news = News::load(std::path::Path::new("/nonexistent"));
    news.fetching = true;
    let revision = news.revision;
    let same = news.posts.clone();
    news.took_fetch(Fetched {
        posts: Some(same),
        images: Vec::new(),
        error: None,
    });
    assert!(!news.fetching);
    assert!(news.live);
    assert_eq!(news.revision, revision);
    news.took_fetch(Fetched {
        posts: Some(vec![post("news/new", "2026-11-18")]),
        images: Vec::new(),
        error: None,
    });
    assert_eq!(news.revision, revision.wrapping_add(1));
    assert_eq!(news.posts.len(), 1);
}
