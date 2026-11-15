//! Pages of any server (issue #785): who provides a page for Settings › Privacy ([`provider`]), and – with strand 2b
//! of issue #784 – its card through the own Nextcloud's references (`resolve_reference`, moved here from
//! `crate::chat::attachments`) and the privacy decision for it, for the chat with people and the assistant alike.

/// The provider of `link` as Settings › Privacy names it: its host as the suite compares hosts
/// ([`torro_core::privacy::normalize_host`]); empty for an address without one.
pub fn provider(link: &str) -> String {
    let host = url::Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_default();
    torro_core::privacy::normalize_host(&host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_is_the_host() {
        assert_eq!(provider("https://example.org/a?b"), torro_core::privacy::normalize_host("example.org"));
        assert_eq!(provider("not an address"), "");
    }
}
