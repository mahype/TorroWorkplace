//! Synthetic, offline workflow tests; real invoice content never belongs in fixtures or diagnostics.

use super::invoice;

const AUGUST: &str = include_str!("../../tests/fixtures/workflows/paddle_august.html");
const OCTOBER: &str = include_str!("../../tests/fixtures/workflows/paddle_october.html");

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
    for html in [AUGUST, OCTOBER] {
        let copied = invoice::convert(html.as_bytes().to_vec(), Some("text/html; charset=utf-8".into()), None)
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
    let paged = invoice::convert(long.into_bytes(), Some("text/html; charset=utf-8".into()), None)
        .await
        .expect("complete multiple pages");
    assert!(paged.pages > 1, "all pages are independently marked and round-trip checked");
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    assert!(matches!(
        invoice::convert(AUGUST.as_bytes().to_vec(), Some("text/html; charset=utf-8".into()), Some(cancelled)).await,
        Err(invoice::Error::Cancelled)
    ));
    invoice::verify_worker_admission_after_cancellation().await;
}

/// Opt-in local acceptance only. Private source and generated documents never enter test output or the repository.
#[tokio::test]
#[ignore = "requires the person's two private Paddle examples in a local temporary directory"]
async fn local_paddle_acceptance() {
    let dir = std::env::var_os("TORRO_WORKFLOW_PADDLE_ACCEPTANCE").expect("explicit private temporary directory");
    let dir = std::path::PathBuf::from(dir);
    for name in ["august", "october"] {
        let bytes = std::fs::read(dir.join(format!("{name}.html"))).expect("local source");
        let source = invoice::extract(&bytes, Some("text/html; charset=utf-8")).expect("complete source profile");
        let copy =
            invoice::convert(bytes, Some("text/html; charset=utf-8".into()), None).await.expect("complete PDF copy");
        std::fs::write(dir.join(format!("{name}.source.txt")), source.text()).expect("local comparison source");
        std::fs::write(dir.join(format!("{name}.pdf")), copy.bytes).expect("local PDF for all-page inspection");
        assert!(copy.pages > 0 && copy.pages <= 50);
    }
}

/// The chosen safe-abort boundary for the current private provider pages; this is NOT a positive acceptance.
#[tokio::test]
#[ignore = "requires explicit local access to the person's private Paddle examples"]
async fn local_paddle_unproven_resources_are_rejected() {
    let dir = std::env::var_os("TORRO_WORKFLOW_PADDLE_ACCEPTANCE").expect("explicit private temporary directory");
    let dir = std::path::PathBuf::from(dir);
    for name in ["august", "october"] {
        let bytes = std::fs::read(dir.join(format!("{name}.html"))).expect("local source");
        assert!(
            matches!(
                invoice::convert(bytes, Some("text/html; charset=utf-8".into()), None).await,
                Err(invoice::Error::Unsupported)
            ),
            "unproven provider resources must not produce a success PDF"
        );
    }
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
