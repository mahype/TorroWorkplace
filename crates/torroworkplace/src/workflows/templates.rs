//! Bundled, versioned provider packages. Reading or copying a template never executes it or grants access.

use sha2::{Digest, Sha256};
use torro_ipc::workflows::{InvoiceProfile, Package, Step, TemplateOrigin, WorkflowTemplate};

use super::{Error, invoice};

/// Service-owned labels remain static so the UI's initial draft language can be translated safely.
pub(super) fn labels(id: &str) -> Result<(&'static str, &'static str), Error> {
    match id {
        "direct_pdf" => Ok((
            "Direct PDF",
            "Uses one unambiguous direct PDF link in the selected mail. The draft binds its exact host from that mail; no login or resource loading.",
        )),
        "stripe_invoice" => Ok((
            "Stripe invoice",
            "Retrieves an original invoice PDF through the bounded Stripe JSON resolver. No receipt, payment, login or browser actions; expired links need manual review.",
        )),
        "paddle_setapp_invoice_copy" => Ok((
            "Paddle / Setapp invoice copy",
            "Creates a marked PDF copy only of supported Paddle HTML without stylesheets or scripts. Current provider pages using external resources are unsupported; no resource loading or login.",
        )),
        _ => Err(Error::NotFound),
    }
}

pub fn list() -> Result<Vec<WorkflowTemplate>, Error> {
    let direct = Package::direct_pdf("files.example.org");
    let stripe = Package {
        hosts: vec![
            "invoice.stripe.com".into(),
            "invoicedata.stripe.com".into(),
            "stripe-upload-api.s3.us-west-1.amazonaws.com".into(),
        ],
        steps: vec![
            Step::MailLink { host: "invoice.stripe.com".into(), label: None },
            Step::ResolveStripeInvoicePdf { version: 1 },
            Step::VerifyPdf,
            Step::SaveToTarget,
        ],
        ..direct.clone()
    };
    let mut paddle = Package {
        hosts: vec!["my.paddle.com".into()],
        steps: vec![
            Step::MailLink { host: "my.paddle.com".into(), label: None },
            Step::Get,
            Step::RenderInvoiceCopy { profile: InvoiceProfile::PaddleSetapp, version: 1 },
            Step::VerifyPdf,
            Step::SaveToTarget,
        ],
        ..direct.clone()
    };
    paddle.binding.typesetter = 1;
    paddle.binding.fonts_digest = invoice::binding();
    [("direct_pdf", direct), ("stripe_invoice", stripe), ("paddle_setapp_invoice_copy", paddle)]
        .into_iter()
        .map(|(id, package)| {
            let (name, description) = labels(id)?;
            let revision = 1u32;
            let encoded =
                serde_json::to_vec(&(id, revision, name, description, &package)).map_err(|_| Error::Blocked)?;
            Ok(WorkflowTemplate {
                id: id.into(),
                revision,
                digest: Sha256::digest(encoded).iter().map(|byte| format!("{byte:02x}")).collect(),
                name: name.into(),
                description: description.into(),
                package,
            })
        })
        .collect()
}

pub fn get(id: &str) -> Result<WorkflowTemplate, Error> {
    list()?.into_iter().find(|template| template.id == id).ok_or(Error::NotFound)
}

/// Instantiation resolves only a local mail-derived host; it makes no DNS or HTTP requests.
pub fn instantiate(
    id: &str,
    revision: u32,
    digest: &str,
    direct_host: Option<&str>,
) -> Result<(Package, TemplateOrigin), Error> {
    let template = get(id)?;
    if template.revision != revision || template.digest != digest {
        return Err(Error::Conflict);
    }
    let mut package = template.package;
    if id == "direct_pdf" {
        let host = direct_host.filter(|host| torro_ipc::workflows::valid_host(host)).ok_or(Error::Invalid)?;
        package.hosts = vec![host.into()];
        let first = package.steps.first_mut().ok_or(Error::Blocked)?;
        *first = Step::MailLink { host: host.into(), label: None };
    }
    package.validate().map_err(|_| Error::Unsupported)?;
    Ok((package, TemplateOrigin { id: template.id, revision, digest: template.digest }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_packages_are_usable_pinned_and_contain_no_private_examples() {
        let templates = list().expect("bundled templates");
        assert_eq!(templates.len(), 3);
        for template in templates {
            assert!(template.package.validate().is_ok());
            let (package, origin) =
                instantiate(&template.id, template.revision, &template.digest, Some("documents.example.org"))
                    .expect("inactive package");
            assert_eq!(origin.digest, template.digest);
            assert!(package.validate().is_ok());
            assert!(!serde_json::to_string(&package).expect("package").contains("://"));
            assert_eq!(instantiate(&template.id, 2, &template.digest, None), Err(Error::Conflict));
        }
    }
}
