//! Synthetic, offline workflow tests; real invoice content never belongs in fixtures or diagnostics.

use super::download::ResourceKind;
use super::invoice::{self, Loaded, Reference};

const AUGUST: &str = include_str!("../../tests/fixtures/workflows/paddle_august.html");
const OCTOBER: &str = include_str!("../../tests/fixtures/workflows/paddle_october.html");
const CSS: &str = include_str!("../../tests/fixtures/workflows/paddle.css");
const STYLESHEET: &str = "https://static.paddle.com/assets/css/customer/invoice.css";
const SCRIPT: &str = "https://static.paddle.com/assets/js/customer/invoice.js";
const HTML: &str = "text/html; charset=utf-8";

/// A synthetic profile-2 page: the profile-1 fixture with the provider's head resources.
pub(super) fn with_resources(html: &str) -> String {
    html.replace(
        "</head>",
        &format!("<link rel=\"stylesheet\" href=\"{STYLESHEET}\"><script defer src=\"{SCRIPT}\"></script></head>"),
    )
}

fn loaded(css: &str) -> Vec<Loaded> {
    vec![
        Loaded {
            reference: Reference { kind: ResourceKind::Stylesheet, href: STYLESHEET.into() },
            stylesheet: Some(css.as_bytes().to_vec()),
        },
        Loaded { reference: Reference { kind: ResourceKind::Script, href: SCRIPT.into() }, stylesheet: None },
    ]
}

#[test]
fn invoice_profiles_preserve_every_source_block_and_cell_relationship() {
    for (html, expected) in [
        (AUGUST, include_str!("../../tests/fixtures/workflows/paddle_august.expected.txt")),
        (OCTOBER, include_str!("../../tests/fixtures/workflows/paddle_october.expected.txt")),
    ] {
        let parsed = invoice::extract(html.as_bytes(), Some("text/html; charset=utf-8")).expect("profile");
        assert_eq!(parsed.text(), expected.trim_end());
        assert!(!parsed.text().contains("Edit receipt information"));
        assert!(!parsed.text().contains("synthetic-nonsecret"));
    }
}

#[tokio::test]
async fn invoice_pdfs_round_trip_all_content_and_mark_every_page_as_a_copy() {
    let _turn = invoice::TEST_TURN.lock().await;
    for html in [AUGUST, OCTOBER] {
        let copied = invoice::convert(html.as_bytes().to_vec(), Some("text/html; charset=utf-8".into()), None, None)
            .await
            .expect("complete copy");
        assert!(copied.bytes.starts_with(b"%PDF-"));
        assert!(copied.pages > 0 && copied.pages <= 50);
    }
    let long = AUGUST.replace(
        "<h4>Payment details</h4>",
        &format!(
            "{}<h4>Payment details</h4>",
            "<p>Synthetic complete line with measured text and currency €: no shortening.</p>".repeat(70)
        ),
    );
    let paged = invoice::convert(long.into_bytes(), Some("text/html; charset=utf-8".into()), None, None)
        .await
        .expect("complete multiple pages");
    assert!(paged.pages > 1, "all pages are independently marked and round-trip checked");
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    assert!(matches!(
        invoice::convert(AUGUST.as_bytes().to_vec(), Some("text/html; charset=utf-8".into()), None, Some(cancelled))
            .await,
        Err(invoice::Error::Cancelled)
    ));
    invoice::verify_worker_admission_after_cancellation().await;
}

/// Opt-in local acceptance only. Private source and generated documents never enter test output or the repository.
/// The directory holds `<name>.html` for each page and `res/` with every resource the pages name, under its file
/// name. Only resource hosts, paths and script digests are printed, never invoice content.
#[tokio::test]
#[ignore = "requires the person's private Paddle examples in a local temporary directory"]
async fn local_paddle_acceptance() {
    let _turn = invoice::TEST_TURN.lock().await;
    use sha2::{Digest, Sha256};
    let dir = std::env::var_os("TORRO_WORKFLOW_PADDLE_ACCEPTANCE").expect("explicit private temporary directory");
    let dir = std::path::PathBuf::from(dir);
    let template = super::templates::get("paddle_setapp_invoice_copy").expect("bundled template").package;
    let names = std::env::var("TORRO_WORKFLOW_PADDLE_NAMES").unwrap_or_else(|_| "august october".into());
    for name in names.split_whitespace() {
        let bytes = std::fs::read(dir.join(format!("{name}.html"))).expect("local source");
        let (source, presentation) = invoice::extract_profile_2(&bytes, Some(HTML)).expect("complete source profile");
        for sha256 in &presentation.inline_scripts {
            let pinned = template.binding.resources.iter().any(|pin| pin.inline && pin.sha256 == *sha256);
            eprintln!("{name}: inline script {sha256} pinned={pinned}");
        }
        let mut resources = Vec::new();
        for reference in presentation.references {
            let link = super::download::resource_link("my.paddle.com", &reference.href, &template.hosts)
                .expect("resource on a package host");
            let file = link.path_and_query().rsplit('/').next().unwrap_or_default().to_owned();
            let data = std::fs::read(dir.join("res").join(&file)).expect("local resource");
            let sha256: String = Sha256::digest(&data).iter().map(|byte| format!("{byte:02x}")).collect();
            let pinned = template.binding.resources.iter().any(|pin| {
                !pin.inline && pin.host == link.host() && pin.path == link.path_and_query() && pin.sha256 == sha256
            });
            eprintln!(
                "{name}: {:?} {} {} {sha256} pinned={pinned}",
                reference.kind,
                link.host(),
                link.path_and_query()
            );
            let stylesheet = (reference.kind == ResourceKind::Stylesheet).then_some(data);
            resources.push(Loaded { reference, stylesheet });
        }
        let copy = invoice::convert(bytes, Some(HTML.into()), Some(resources), None).await.expect("complete PDF copy");
        std::fs::write(dir.join(format!("{name}.source.txt")), source.text()).expect("local comparison source");
        std::fs::write(dir.join(format!("{name}.pdf")), copy.bytes).expect("local PDF for all-page inspection");
        eprintln!("{name}: {} page(s)", copy.pages);
        assert!(copy.pages > 0 && copy.pages <= 50);
    }
}

#[tokio::test]
async fn profile_two_copies_a_page_whose_checked_resources_match_its_head_exactly() {
    let _turn = invoice::TEST_TURN.lock().await;
    for (html, expected) in [
        (AUGUST, include_str!("../../tests/fixtures/workflows/paddle_august.expected.txt")),
        (OCTOBER, include_str!("../../tests/fixtures/workflows/paddle_october.expected.txt")),
    ] {
        let page = with_resources(html);
        let (parsed, presentation) = invoice::extract_profile_2(page.as_bytes(), Some(HTML)).expect("profile 2");
        assert_eq!(parsed.text(), expected.trim_end(), "the resources change nothing in the extracted invoice");
        assert_eq!(presentation.references, loaded(CSS).into_iter().map(|loaded| loaded.reference).collect::<Vec<_>>());
        let copy = invoice::convert(page.into_bytes(), Some(HTML.into()), Some(loaded(CSS)), None)
            .await
            .expect("complete copy");
        assert!(copy.bytes.starts_with(b"%PDF-") && copy.pages > 0);
    }
    let page = with_resources(AUGUST);
    // Profile 1 still refuses any resource, and profile 2 only follows the exact loaded list.
    assert!(invoice::extract(page.as_bytes(), Some(HTML)).is_err());
    assert!(invoice::convert(page.clone().into_bytes(), Some(HTML.into()), None, None).await.is_err());
    let mut missing = loaded(CSS);
    missing.pop();
    assert!(invoice::convert(page.clone().into_bytes(), Some(HTML.into()), Some(missing), None).await.is_err());
    let mut swapped = loaded(CSS);
    swapped.reverse();
    assert!(invoice::convert(page.clone().into_bytes(), Some(HTML.into()), Some(swapped), None).await.is_err());
    let mut unloaded = loaded(CSS);
    unloaded[0].stylesheet = None;
    assert!(invoice::convert(page.into_bytes(), Some(HTML.into()), Some(unloaded), None).await.is_err());
    // A page without resources stays valid under profile 2; its list is simply empty.
    assert!(invoice::extract_profile_2(AUGUST.as_bytes(), Some(HTML)).expect("plain page").1.references.is_empty());
}

#[tokio::test]
async fn generated_or_hidden_invoice_content_in_the_stylesheet_is_refused() {
    let _turn = invoice::TEST_TURN.lock().await;
    let page = with_resources(AUGUST);
    for rule in [
        ".receipt-table-total-right::after { content: 'Additional VAT: EUR 9.00'; }",
        ".receipt-table-total-right::before { content: attr(data-vat); }",
        "@import url(\"https://static.paddle.com/more.css\");",
        "@import \"more.css\";",
        ".receipt-table-total { display: none; }",
        ".receipt-table-total { DISPLAY: NONE !important; }",
        "@media print { .receipt-table-total { display: none; } }",
        ".no-print, .receipt-table-total { display: none; }",
        ".receipt-table-tax { visibility: hidden; }",
        ".receipt-table-tax { opacity: 0; }",
        ".receipt-table-tax { font-size: 0; }",
        ".receipt-table-tax { font: 0/0 a; }",
        ".receipt-table-tax { color: transparent; }",
        ".receipt-table-tax { color: rgba(0, 0, 0, 0); }",
        ".receipt-table-tax { --gone: none; display: var(--gone); }",
        ".receipt-table-tax { color: var(--text); }",
        ".receipt-table-tax { clip: rect(0 0 0 0); }",
        ".receipt-table-tax { clip-path: inset(50%); }",
        ".receipt-table-tax { text-indent: -9999px; }",
        ".receipt-table-tax { position: absolute; left: -10000px; }",
        ".receipt-table-tax { unicode-bidi: bidi-override; direction: rtl; }",
        ".receipt-table-tax { animation: gone 1s; }",
        "@namespace svg url(http://www.w3.org/2000/svg);",
        ".receipt-table { .receipt-table-total { display: none; } }",
        ".receipt-table-total { left: -99999px; }",
        ".receipt-table-total { position: relative; left: -99999px; }",
        ".receipt-table-total { margin-left: -99999px; }",
        ".receipt-table-total { text-indent: -9999pt; }",
        ".receipt-table-total { text-indent: -100vw; }",
        ".receipt-table-total { font: 700 0px a; }",
        ".receipt-table-total { font-size: 0.01px; }",
        ".receipt-table-total { color: #0000; }",
        ".receipt-table-total { color: #ffffff00; }",
        ".receipt-table-total { transform: scale(0); }",
        ".receipt-table-total { transform: translateX(-99999px); }",
        ".receipt-table-total { content-visibility: hidden; }",
        ".receipt-table-total { filter: opacity(0); }",
        ".receipt-table-total { height: 0; overflow: hidden; }",
        ".receipt-table-total { -webkit-text-security: disc; }",
        ".receipt-table-tax { display: list-item; }",
        ".receipt-table-tax { list-style: 'VAT 19 % ' inside; }",
        ".receipt-table-tax { background-image: url(\"data:image/svg+xml,<svg><text>VAT</text></svg>\"); }",
        ".receipt-table-tax { background: url(data:image/png;base64,AAAA); }",
        ".receipt-table-total { left: 99999px; }",
        ".receipt-table-total { margin-left: 99999px; }",
        ".receipt-table-total { transform: matrix(0, 0, 0, 0, 0, 0); }",
        ".receipt-table-total { -webkit-transform: scale(0); }",
        ".receipt-table-total { rotate: y 90deg; }",
        ".receipt-table-total { zoom: 1%; }",
        ".receipt-table-total { color: rgb(0 0 0 / 1%); }",
        ".receipt-table-total { color: #ffffff0a; }",
        ".receipt-table-total { font-size: 5%; }",
        ".receipt-table-total { -webkit-mask-image: linear-gradient(transparent, transparent); }",
        ".receipt-table-total { overflow: hidden; }",
        ".receipt-table-total { max-height: 1.2em; overflow: hidden; }",
        ".receipt-table-total { background-image: image-set(\"data:image/svg+xml,<svg/>\" 1x); }",
        ".receipt-table-total { background: url(https://static.paddle.com/vat.png); }",
        ".no-print ~ .receipt-table-total { display: none; }",
        ":root { --z: 0; }",
        "@font-face { font-family: x; src: url(data:font/woff2;base64,AAAA); }",
        // Selector forms a browser reads differently from a naive split (review of the allow list).
        "*|p, *|h1, *|*.receipt-table-total { display: none; }",
        ":not([x=\"]) .zzz\"]) p { display: none; }",
        ".zzz[x=\"/*\"], .receipt-table-total, [y=\"*/\"] { display: none; }",
        ".no-print [x=\"/*\"], .receipt-table-total, [y=\"*/\"] { display: none; }",
        "tbody .receipt-table-total, body .receipt-table-total { display: none; }",
        ".RECEIPT-TABLE-TOTAL { display: none; }",
        // Values that hide, cover or move text.
        ".receipt-table-total { display: table-column; }",
        ".c-badge { outline: 3000px solid #fff; }",
        ".c-badge { position: relative; box-shadow: 0 0 0 3000px #fff; }",
        ".c-sheet { position: relative; z-index: -1; }",
        ".receipt-table-head { position: relative; top: 400px; }",
        ".receipt-table-total-right { text-decoration: line-through 2em #fff; }",
        ".receipt-table-total-right { text-indent: calc(-200px / 0.001); }",
        ".c-sheet div { font-size: 50%; }",
        ".c-sheet div { font-size: smaller; }",
        ".receipt-table-total-right { padding-left: 50000px; }",
        ".receipt-table-total { font: 12px/1px a; }",
        ".receipt-table-total { letter-spacing: -8px; }",
        "@page { @bottom-center { content: 'Additional VAT'; } }",
        ".c-header { position: fixed; top: 0; left: 0; width: 100%; height: 100%; background-color: #fff; z-index: 999; }",
        // Final review: brackets a browser closes differently, url() tokens, constants and relative units.
        ".receipt-table-total:not(:is(x]].zzz)) { display: none; }",
        ".zzz:is(url(a[)), .receipt-table-total { display: none; }",
        ".zzz:is(url(/*)), .receipt-table-total, :is(url(*/)) { display: none; }",
        ".receipt-table-head { position: relative; top: calc(infinity * 1px); }",
        ".receipt-table-head { position: relative; left: calc(-infinity * 1px); }",
        ".c-badge { outline: 0.5rem solid #fff; }",
        ".receipt-table-total-right { text-decoration: line-through 0.25rem #fff; }",
        ".receipt-table-total { letter-spacing: -0.0625rem; }",
        ".receipt-table-total { outline: 0.5lh solid #fff; }",
        ".c-sheet div { font-size: 0.5ex; }",
        ".c-sheet div { font-size: 0.5rem; }",
        ".c-sheet div { font: smaller sans-serif; }",
        ".c-sheet div { font-size: larger; }",
        "html { font-size: 1000px; }",
        ".receipt-table-total { line-height: 10000px; }",
        "@page { margin: 5000px; }",
        "@page { size: 10px 10px; }",
        ".receipt-table-total { color: #aa\u{e4}; }",
        // Offsets that stay within the bound per element but add up through nesting.
        ".c-sheet div { position: relative; top: -64px; }",
        ".c-sheet div, .c-sheet section { margin-left: -100px; }",
        ".g-container { margin-top: -100px; } .g-row { margin-top: -100px; }",
    ] {
        let css = format!("{CSS}\n{rule}\n");
        assert!(invoice::check_stylesheet(css.as_bytes(), page.as_bytes()).is_err(), "{rule}");
        assert!(
            invoice::convert(page.clone().into_bytes(), Some(HTML.into()), Some(loaded(&css)), None).await.is_err(),
            "{rule}"
        );
    }
    for rule in [
        ".no-print { display: none; }",
        "@media print { .c-header { display: none !important; } }",
        "/* nav */ .no-print /* edit */ , .c-header { visibility: hidden; }",
        ".receipt-table-total { color: #123456; font-weight: 700; }",
        ".receipt-table-total::after { content: none; }",
        ".c-header { position: fixed; top: -64px; }",
        ".receipt-table-total { margin: 0 auto -8px; font: 700 14px/1.4 sans-serif; color: #11663aff; }",
        "hr { height: 0; overflow: visible; }",
        "ul { list-style: none; }",
        ".no-print * { display: none; }",
        ".c-header > .c-container { visibility: hidden; }",
        // Rules that name a class, id or tag the page lacks cannot change it, whatever they do.
        ".select2-hidden-accessible { clip: rect(0 0 0 0); position: absolute; overflow: hidden; }",
        ".ui-helper-hidden { display: none; }",
        "template, [hidden] template { display: none; }",
        "#not-on-the-page::after { content: 'Additional VAT'; }",
        "@keyframes gone { to { opacity: 0; } }",
        // A skipped block is consumed: the next selector must not start inside it (seen on Paddle's stylesheet).
        ".not-on-the-page { padding-top: 0 }.no-print, .no-print * { display: none }",
        "@page { margin: 1cm }.c-header { display: none }",
        ".zzz[x=\"a, .receipt-table-total\"] { display: none; }",
        ".receipt-table-total { font: 700 14px/20px sans-serif; letter-spacing: 0.5px; z-index: 9; }",
        ".c-sheet { box-shadow: 2px 2px 8px 0 rgba(210, 212, 222, 0.8); border-bottom: 1px solid #d2d4de; }",
        ".c-badge { position: relative; top: -.5rem; } .g-row { margin-left: calc(2rem * 0.5 * -1); }",
        ".receipt-table-total { margin-left: calc(2rem * 0.5 * -1); color: rgba(54, 71, 108, 0.9); }",
    ] {
        let css = format!("{CSS}\n{rule}\n");
        assert!(invoice::check_stylesheet(css.as_bytes(), page.as_bytes()).is_ok(), "{rule}");
    }
    assert!(invoice::check_stylesheet(CSS.as_bytes(), page.as_bytes()).is_ok());
    let big = format!("{CSS}{}", ".a{color:red}".repeat(512 * 1024 / 13 + 1));
    assert!(invoice::check_stylesheet(big.as_bytes(), page.as_bytes()).is_err(), "over 512 KiB");
    let many = ".a{color:red}".repeat(20_001);
    assert!(invoice::check_stylesheet(many.as_bytes(), page.as_bytes()).is_err(), "over 20,000 rules");
    assert!(invoice::check_stylesheet(&[0xff, 0xfe], page.as_bytes()).is_err(), "not UTF-8");
}

#[tokio::test]
async fn profile_two_accepts_only_witnessed_page_code() {
    let _turn = invoice::TEST_TURN.lock().await;
    // Inline code is accepted structurally, then checked: styles by the stylesheet rules, scripts by their pin.
    let page = AUGUST.replace(
        "</head>",
        "<link rel=\"icon\" href=\"/favicon.ico\"><meta name=\"robots\" content=\"noindex\">\
         <style type=\"text/css\">@font-face { font-family: 'Graphik'; src: url('/f.woff') format('woff'); }</style>\
         <script type=\"text/javascript\">void 0;</script></head>",
    );
    let (_, presentation) = invoice::extract_profile_2(page.as_bytes(), Some(HTML)).expect("witnessed page code");
    assert!(presentation.references.is_empty());
    assert_eq!(presentation.inline_scripts.len(), 1);
    assert!(invoice::references(page.clone().into_bytes(), Some(HTML.into()), None).await.is_ok());
    assert!(invoice::convert(page.into_bytes(), Some(HTML.into()), Some(Vec::new()), None).await.is_ok());
    let hiding = AUGUST.replace("</head>", "<style>.receipt-table-total { display: none; }</style></head>");
    assert!(invoice::references(hiding.clone().into_bytes(), Some(HTML.into()), None).await.is_err());
    assert!(invoice::convert(hiding.into_bytes(), Some(HTML.into()), Some(Vec::new()), None).await.is_err());
    let script = AUGUST.replace("</main>", "</main><script type=\"text/javascript\">void 0;</script>");
    assert_eq!(invoice::extract_profile_2(script.as_bytes(), Some(HTML)).expect("outside").1.inline_scripts.len(), 1);
    // A `style` attribute outside the invoice gets the stylesheet rules: no overlay without a stylesheet.
    let cover =
        AUGUST.replace("</main>", "</main><div style=\"position:fixed;inset:0;background:#fff;z-index:9\"></div>");
    assert!(invoice::references(cover.clone().into_bytes(), Some(HTML.into()), None).await.is_err());
    assert!(invoice::convert(cover.into_bytes(), Some(HTML.into()), Some(Vec::new()), None).await.is_err());
    let spacing = AUGUST.replace("</main>", "</main><div style=\"margin-top: 3rem\"></div>");
    assert!(invoice::references(spacing.into_bytes(), Some(HTML.into()), None).await.is_ok());
    // Inside the witnessed navigation a `style` attribute may hide, as the navigation's own rules may.
    let navigation = AUGUST.replace(
        "<header class=\"c-header no-print\">",
        "<header class=\"c-header no-print\"><span style=\"display:none\"></span>",
    );
    assert!(invoice::references(navigation.into_bytes(), Some(HTML.into()), None).await.is_ok());
    let hidden = AUGUST.replace("</main>", "</main><div style=\"display:none\"></div>");
    assert!(invoice::references(hidden.into_bytes(), Some(HTML.into()), None).await.is_err());
    let inside = AUGUST.replace("<h4>Payment details</h4>", "<script>void 0;</script><h4>Payment details</h4>");
    assert!(invoice::extract_profile_2(inside.as_bytes(), Some(HTML)).is_err(), "no script inside the invoice");
    for inserted in [
        "<script src='https://static.paddle.com/a.js'>document.write('x')</script>",
        "<script type='module'>import('x')</script>",
        "<style media='print'>.a{}</style>",
        "<meta http-equiv='refresh' content='0'>",
        "<meta name='viewport' content='x'>",
        "<script type='module' src='https://static.paddle.com/a.js'></script>",
        "<script src='https://static.paddle.com/a.js' onload='x()'></script>",
        "<script src='https://static.paddle.com/a.js' integrity='sha256-x'></script>",
        "<link rel='stylesheet' href='https://static.paddle.com/a.css' media='print'>",
        "<link rel='preload' href='https://static.paddle.com/a.css'>",
        "<link rel='stylesheet'>",
        "<base href='https://evil.example/'>",
    ] {
        let html = AUGUST.replace("</head>", &format!("{inserted}</head>"));
        assert!(invoice::extract_profile_2(html.as_bytes(), Some(HTML)).is_err(), "{inserted}");
    }
    let body = AUGUST.replace("</main>", &format!("</main><script src=\"{SCRIPT}\"></script>"));
    assert!(invoice::extract_profile_2(body.as_bytes(), Some(HTML)).is_err(), "external scripts outside the head");
    let nine = (0..9).map(|n| format!("<script src=\"/{n}.js\"></script>")).collect::<String>();
    let html = AUGUST.replace("</head>", &format!("{nine}</head>"));
    assert!(invoice::extract_profile_2(html.as_bytes(), Some(HTML)).is_err(), "at most eight resources");
}

#[test]
fn invoice_profile_rejects_unsafe_or_ambiguous_content_instead_of_dropping_it() {
    for inserted in [
        "<input value='a relevant invoice value'>",
        "<img src='https://example.org/amount.png' alt='Total: 100'>",
        "<div hidden>Additional charge: 100</div>",
        "<div style='display:none'>Additional charge: 100</div>",
        "<svg><text>Additional charge: 100</text></svg>",
        "<iframe src='https://example.org/invoice'></iframe>",
        "<p>Unsupported text: \u{202e}1234</p>",
    ] {
        let html = AUGUST.replace("<h4>Payment details</h4>", inserted);
        assert!(invoice::extract(html.as_bytes(), Some("text/html; charset=utf-8")).is_err(), "{inserted}");
    }
    assert!(invoice::extract(AUGUST.as_bytes(), Some("text/html; charset=unknown")).is_err());
    assert!(invoice::extract(AUGUST.as_bytes(), Some("text/html; charset=windows-1252")).is_err());
    assert!(invoice::extract(AUGUST.as_bytes(), Some("text/html; charset=utf-8; charset=windows-1252")).is_err());
    let equiv = AUGUST.replace(
        "<meta charset=\"utf-8\">",
        "<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\">",
    );
    assert!(invoice::extract(equiv.as_bytes(), Some("text/html; charset=utf-8")).is_err());
    assert!(invoice::extract(&[0xff, 0xff], Some("text/html; charset=utf-8")).is_err());
    let deep = format!("{}x{}", "<div>".repeat(65), "</div>".repeat(65));
    assert!(invoice::extract(deep.as_bytes(), Some("text/html; charset=utf-8")).is_err());
    let long = format!("<div title='{}'>x</div>", "x".repeat(16 * 1024));
    assert!(invoice::extract(long.as_bytes(), Some("text/html; charset=utf-8")).is_err());
    let long_comment = AUGUST
        .replace("<h4>Payment details</h4>", &format!("<!-- >{}--><h4>Payment details</h4>", "x".repeat(16 * 1024)));
    assert!(invoice::extract(long_comment.as_bytes(), Some("text/html; charset=utf-8")).is_err());
    let attrs = format!("<div {}>x</div>", (0..65).map(|n| format!("a{n}='x' ")).collect::<String>());
    assert!(invoice::extract(attrs.as_bytes(), Some("text/html; charset=utf-8")).is_err());
}

#[test]
fn invoice_head_and_ancestors_cannot_hide_unchecked_presentation_influences() {
    for inserted in [
        "<style>.receipt-table-total-right::after { content: 'Additional VAT: EUR 9.00'; }</style>",
        "<style>@import url(https://static.paddle.com/unversioned.css);</style>",
        "<link rel='stylesheet' href='https://static.paddle.com/assets/css/customer/invoice.css'>",
        "<script src='https://static.paddle.com/assets/js/customer/invoice.js'></script>",
        "<script>document.body.append('Additional VAT: EUR 9.00')</script>",
        "<meta http-equiv='refresh' content='0;url=https://example.org/'>",
        "<input value='Additional VAT: EUR 9.00'>",
    ] {
        let html = AUGUST.replace("</head>", &format!("{inserted}</head>"));
        assert!(
            invoice::extract(html.as_bytes(), Some("text/html; charset=utf-8")).is_err(),
            "unproven head influence must fail closed"
        );
    }
    for replacement in ["<body hidden>", "<body style='display:none'>", "<body onload='changeInvoice()'>"] {
        let html = AUGUST.replace("<body>", replacement);
        assert!(invoice::extract(html.as_bytes(), Some("text/html; charset=utf-8")).is_err());
    }
}
