//! Durable carrier/origin ceilings. Human approval never widens a client's effective rights.

use serde::{Deserialize, Serialize};
use torro_ipc::Request;
use torro_ipc::api::{Access, Area, Rights};

use super::Error;
use crate::approvals::Origin;
use crate::clients::Clients;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub carrier: Option<String>,
    pub origin: Origin,
    pub ceiling: Option<Rights>,
}

impl Scope {
    pub fn suite() -> Self {
        Self { carrier: None, origin: Origin::default(), ceiling: None }
    }

    pub fn owner(&self) -> &str {
        self.carrier.as_deref().unwrap_or("suite")
    }

    pub fn current(&self, clients: &Clients) -> Result<Option<Rights>, Error> {
        let Some(id) = &self.carrier else { return Ok(None) };
        let known = clients.list();
        let own = known.iter().find(|client| &client.id == id).ok_or(Error::Forbidden)?;
        let behalf = self.origin.behalf.as_ref().and_then(|id| known.iter().find(|client| &client.id == id));
        let rights = self.origin.effective(&own.rights, behalf.map(|client| &client.rights)).ok_or(Error::Forbidden)?;
        Ok(Some(match &self.ceiling {
            Some(ceiling) => rights.common(ceiling),
            None => return Err(Error::Forbidden),
        }))
    }
}

pub fn reads(rights: Option<&Rights>, account: &str) -> bool {
    rights.is_none_or(|rights| rights.area(Area::Mail).allows(Access::Read) && rights.reaches_mail_account(account))
}

pub fn target(rights: Option<&Rights>, mail: &str, target: &str) -> bool {
    let request = Request::SaveMailAttachments {
        id: mail.into(),
        parts: Vec::new(),
        to: torro_ipc::mail::SaveTo::Target(target.into()),
    };
    rights.is_none_or(|rights| rights.allows_request(&request))
}

pub fn sees(scope: &Scope, rights: Option<&Rights>, owner: &str, account: &str) -> bool {
    (scope.carrier.is_none() || scope.owner() == owner) && reads(rights, account)
}
