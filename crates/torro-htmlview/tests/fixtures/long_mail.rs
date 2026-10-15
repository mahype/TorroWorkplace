// A mailing-list digest of a bit over 1 MiB, built in code so the repository does not carry a megabyte of
// filler (task M9.6, "lange Mail"): headings, paragraphs and a small bordered table per post, as M9.5 writes
// the table attributes. Included by the tests and the example of torro-htmlview.

/// The long sample mail (cleaned HTML).
pub fn long_mail() -> String {
    let mut out = String::from(
        "<div style=\"font-family:Helvetica,Arial,sans-serif;font-size:15px;line-height:22px;color:#222222;\
         max-width:680px;margin:0 auto;padding:16px\"><h1>Example list digest</h1>",
    );
    let mut post = 0;
    while out.len() < 1_150_000 {
        post += 1;
        out.push_str(&format!(
            "<h3 style=\"margin:24px 0 4px;color:#1f3864\">Post {post}: Re: how do you tile your windows?</h3>\
             <p style=\"margin:0 0 8px\">Sample {post} wrote: I keep the calendar on the left and the inbox \
             in the middle, the tasks on the right. With a wide monitor that leaves room for an editor. \
             The <b>mini tiles</b> are the best part, they show what matters without a click. \
             <a href=\"https://example.com/list/{post}\">Read in the archive</a>.</p>\
             <table style=\"width:100%;border-collapse:collapse;border:1px solid #dddddd;font-size:13px\">\
             <tr><td style=\"width:30%;padding:4px;border:1px solid #dddddd\">Author</td>\
             <td style=\"padding:4px;border:1px solid #dddddd\">Sample {post}</td></tr>\
             <tr><td style=\"padding:4px;border:1px solid #dddddd\">Replies</td>\
             <td style=\"padding:4px;border:1px solid #dddddd\">{}</td></tr></table>",
            post * 7 % 31
        ));
    }
    out.push_str("</div>");
    out
}
