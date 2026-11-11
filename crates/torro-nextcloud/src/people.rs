//! The person behind the account and the search for other users of the server (#643).
//!
//! - Who the account is: `GET /ocs/v2.php/cloud/user` (provisioning API, operation
//!   `users-get-current-user`): `id` (the user id, which the login name need not be),
//!   `displayname` (older servers `display-name`) and `quota.free`
//!   (<https://github.com/nextcloud/server/blob/master/apps/provisioning_api/openapi.json>).
//! - Sharee search, the search of the sharing dialog; works without Talk:
//!   `GET /ocs/v2.php/apps/files_sharing/api/v1/sharees?search=…&itemType=file&shareType[]=0&perPage=…&lookup=false`,
//!   users in `exact.users` and `users`, each with `label` and `value.shareWith`
//!   (<https://github.com/nextcloud/server/blob/master/apps/files_sharing/openapi.json>,
//!   schema `ShareesSearchResult`).
//! - Autocomplete as Talk's "new conversation" uses it:
//!   `GET /ocs/v2.php/core/autocomplete/get?search=…&itemType=call&itemId=new&shareTypes[]=0&limit=…`,
//!   entries with `id`, `label` and `source` (only `users` are kept)
//!   (<https://github.com/nextcloud/server/blob/master/core/openapi.json>, schema
//!   `AutocompleteResult`).
//!
//! Both searches follow the admin's settings for who may find whom; an empty answer can mean
//! that the server shows no other users.

use std::collections::HashSet;

use reqwest::Method;
use serde_json::Value;

use crate::notifications::Person;
use crate::ocs::{Nextcloud, OcsError};
use crate::rich;

/// The most people one search asks for.
const SEARCH_MAX: u32 = 100;

/// The person the account belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The user id, one line: what DAV paths and mentions use (the login name may differ).
    pub id: String,
    /// The display name, one line; the id when the server sends none.
    pub display_name: String,
    /// Free space in bytes (`quota.free`), when the server names a number of 0 or more.
    pub quota_free: Option<i64>,
}

impl Nextcloud {
    /// Who the account is. [`OcsError::Other`] when the answer names no user id.
    pub async fn user(&self) -> Result<User, OcsError> {
        let url = self.endpoint::<&str>("/ocs/v2.php/cloud/user", &[], &[])?;
        let data = self.exchange(Method::GET, url, None, None).await?.success()?;
        data.as_ref().and_then(read_user).ok_or_else(|| OcsError::Other("Nextcloud sent no user".into()))
    }

    /// Up to `limit` (1 to 100) users whose name or id `search` finds, through the sharee search;
    /// exact hits first, each id once.
    pub async fn find_sharees(&self, search: &str, limit: u32) -> Result<Vec<Person>, OcsError> {
        let limit = limit.clamp(1, SEARCH_MAX);
        let query = [
            ("search", search.to_string()),
            ("itemType", "file".to_string()),
            ("shareType[]", "0".to_string()),
            ("perPage", limit.to_string()),
            ("lookup", "false".to_string()),
        ];
        let url = self.endpoint("/ocs/v2.php/apps/files_sharing/api/v1/sharees", &[], &query)?;
        let data = self.exchange(Method::GET, url, None, None).await?.success()?;
        Ok(data.as_ref().map(|data| read_sharees(data, limit)).unwrap_or_default())
    }

    /// Up to `limit` (1 to 100) users whose name or id `search` finds, through the autocomplete
    /// Talk uses for a new conversation; each id once.
    pub async fn find_people(&self, search: &str, limit: u32) -> Result<Vec<Person>, OcsError> {
        let query = [
            ("search", search.to_string()),
            ("itemType", "call".to_string()),
            ("itemId", "new".to_string()),
            ("shareTypes[]", "0".to_string()),
            ("limit", limit.clamp(1, SEARCH_MAX).to_string()),
        ];
        let url = self.endpoint("/ocs/v2.php/core/autocomplete/get", &[], &query)?;
        let data = self.exchange(Method::GET, url, None, None).await?.success()?;
        let entries = data.as_ref().and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        let mut seen = HashSet::new();
        Ok(entries
            .iter()
            .filter(|entry| entry.get("source").and_then(Value::as_str) == Some("users"))
            .filter_map(|entry| person(entry.get("id"), entry.get("label")))
            .filter(|person| seen.insert(person.id.clone()))
            .collect())
    }
}

fn read_user(data: &Value) -> Option<User> {
    let line = |key: &str| data.get(key).and_then(Value::as_str).map(|text| rich::line(text, &Value::Null));
    let id = line("id").filter(|id| !id.is_empty())?;
    let display_name = line("displayname").or_else(|| line("display-name")).filter(|name| !name.is_empty());
    let free = data.pointer("/quota/free");
    // A byte count may come as a float; whole bytes are enough.
    let quota_free = free.and_then(Value::as_i64).or_else(|| free.and_then(Value::as_f64).map(|f| f as i64));
    Some(User {
        display_name: display_name.unwrap_or_else(|| id.clone()),
        id,
        quota_free: quota_free.filter(|f| *f >= 0),
    })
}

/// The users of a sharee answer, exact hits first, each id once, at most `limit`.
fn read_sharees(data: &Value, limit: u32) -> Vec<Person> {
    let list = |pointer: &str| data.pointer(pointer).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let mut seen = HashSet::new();
    list("/exact/users")
        .iter()
        .chain(list("/users"))
        .filter(|entry| entry.pointer("/value/shareType").and_then(Value::as_i64) == Some(0))
        .filter_map(|entry| person(entry.pointer("/value/shareWith"), entry.get("label")))
        .filter(|person| seen.insert(person.id.clone()))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .collect()
}

/// A person from an id and a label, both made one line; `None` without an id.
fn person(id: Option<&Value>, label: Option<&Value>) -> Option<Person> {
    let id = rich::line(id?.as_str()?, &Value::Null);
    let name = rich::line(label.and_then(Value::as_str).unwrap_or_default(), &Value::Null);
    (!id.is_empty()).then(|| Person { name: if name.is_empty() { id.clone() } else { name }, id })
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use serde_json::json;

    use super::*;
    use crate::ocs::tests::{AUTHORIZATION, account};

    fn ocs(data: Value) -> String {
        json!({"ocs": {"meta": {"status": "ok", "statuscode": 200}, "data": data}}).to_string()
    }

    fn person(id: &str, name: &str) -> Person {
        Person { id: id.into(), name: name.into() }
    }

    #[tokio::test]
    async fn reads_the_current_user() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/cloud/user")
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true");
                then.status(200).body(ocs(json!({
                    "id": "anna", "displayname": "Anna\u{202e} Example", "email": "anna@example.org",
                    "quota": {"free": 5_368_709_120_u64, "used": 1024, "total": 5_368_710_144_u64, "relative": 0.0, "quota": -3},
                })));
            })
            .await;
        let user = account(&server).user().await.expect("the user");
        assert_eq!(
            user,
            User { id: "anna".into(), display_name: "Anna Example".into(), quota_free: Some(5_368_709_120) }
        );
    }

    #[test]
    fn an_old_name_field_a_float_quota_and_no_id() {
        let user = read_user(&json!({"id": "bert", "display-name": "Bert", "quota": {"free": 12.5}})).expect("a user");
        assert_eq!((user.display_name.as_str(), user.quota_free), ("Bert", Some(12)));
        let bare = read_user(&json!({"id": "carla", "quota": {"free": -3}})).expect("a user");
        assert_eq!((bare.display_name.as_str(), bare.quota_free), ("carla", None));
        assert_eq!(read_user(&json!({"displayname": "No id"})), None);
    }

    #[tokio::test]
    async fn finds_sharees_exact_first() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/apps/files_sharing/api/v1/sharees")
                    .query_param("search", "ber")
                    .query_param("itemType", "file")
                    .query_param("shareType[]", "0")
                    .query_param("perPage", "10")
                    .query_param("lookup", "false")
                    .header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(json!({
                    "exact": {"users": [{"label": "Bert Example", "value": {"shareType": 0, "shareWith": "bert"}}],
                              "groups": [], "remotes": [], "remote_groups": [], "emails": [], "circles": [], "rooms": []},
                    "users": [
                        {"label": "Bert Example", "value": {"shareType": 0, "shareWith": "bert"}},
                        {"label": "Berta\nExample", "value": {"shareType": 0, "shareWith": "berta"}},
                        {"label": "Bergen team", "value": {"shareType": 1, "shareWith": "bergen"}},
                        {"label": "No id", "value": {"shareType": 0}},
                    ],
                    "groups": [], "remotes": [], "remote_groups": [], "emails": [], "lookup": [], "circles": [], "rooms": [],
                    "lookupEnabled": false,
                })));
            })
            .await;
        let found = account(&server).find_sharees("ber", 10).await.expect("people");
        assert_eq!(found, vec![person("bert", "Bert Example"), person("berta", "Berta Example")]);
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn finds_people_through_the_autocomplete() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/core/autocomplete/get")
                    .query_param("search", "car")
                    .query_param("itemType", "call")
                    .query_param("itemId", "new")
                    .query_param("shareTypes[]", "0")
                    .query_param("limit", "100")
                    .header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(json!([
                    {"id": "carla", "label": "Carla Example", "icon": "icon-user", "source": "users", "status": "",
                     "subline": "", "shareWithDisplayNameUnique": "carla@example.org"},
                    {"id": "cars", "label": "Cars", "icon": "icon-group", "source": "groups", "status": "", "subline": "",
                     "shareWithDisplayNameUnique": ""},
                    {"id": "carla", "label": "Carla again", "source": "users"},
                ])));
            })
            .await;
        let found = account(&server).find_people("car", 500).await.expect("people");
        assert_eq!(found, vec![person("carla", "Carla Example")]);
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn a_refused_search_is_an_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/ocs/v2.php/core/autocomplete/get");
                then.status(401);
            })
            .await;
        assert_eq!(account(&server).find_people("x", 5).await, Err(OcsError::Refused));
    }
}
