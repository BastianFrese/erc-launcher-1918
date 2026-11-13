use super::feed::{Post, Trouble, embedded, format_date, image_format, merge, parse, version};
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
    assert!(news.body.contains("## Wenn etwas nicht laeuft") || news.body.contains("## Wenn etwas nicht läuft"));
    assert!(posts.windows(2).all(|w| w[0].date >= w[1].date));
}

#[test]
fn no_release_notes_before_one_zero() {
    // Bastians Wort (13.11.): der Changelog bleibt leer, bis der Launcher 1.0.0 ist.
    // Der Test haengt an der eigenen Fassung, nicht an der Nummer der Notiz: ein
    // Merge mit dem Unterbau hat am 13.11. 24 fremde Notizen (1.9.19–1.9.42)
    // wieder hereingetragen — die waren ">= 1.0.0" undrutschten durch.
    let ours = version(&format!("release/{}", env!("CARGO_PKG_VERSION")));
    if ours >= vec![1, 0, 0] {
        // Ab 1.0.0 gehoeren die Notizen wieder ins Programm — dann pruefen wir,
        // dass keine fremde dabei ist: jede muss unsere Fassungsnummer tragen.
        let posts = embedded();
        for note in posts.iter().filter(|p| p.release()) {
            assert!(
                version(&note.id) >= vec![1, 0, 0],
                "fremde Versionsnotiz eingebettet: {}",
                note.version()
            );
        }
        return;
    }
    let posts = embedded();
    let notes: Vec<&Post> = posts.iter().filter(|p| p.release()).collect();
    assert!(
        notes.is_empty(),
        "vor 1.0.0 gehoeren keine Versionsnotizen ins Programm, eingebettet sind: {:?}",
        notes.iter().map(|n| n.version()).collect::<Vec<_>>()
    );
}

#[test]
fn leaves_out_the_changelog_while_it_is_empty() {
    let mut news = News::load(std::path::Path::new("/nonexistent"));
    news.posts.clear();
    news.blocks.clear();
    let mut scroll = NewsScroll::default();
    scroll.sync(&news, false, true, false);
    assert!(scroll.release_indices.is_empty());
    assert!(
        !scroll.stacked_rows.iter().any(|r| matches!(
            r,
            StackedRow::ReleasesHeader | StackedRow::Release(_) | StackedRow::ReleasesBottom
        )),
        "der Changelog-Abschnitt steht da, obwohl es keine Notizen gibt"
    );
    scroll.sync(&news, false, false, false);
    assert!(scroll.release_indices.is_empty());
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
