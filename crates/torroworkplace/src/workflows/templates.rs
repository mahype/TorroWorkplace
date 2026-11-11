//! Bundled, versioned provider packages. Reading or copying a template never executes it or grants access.

use sha2::{Digest, Sha256};
use torro_ipc::workflows::{InvoiceProfile, Package, ResourcePin, Step, TemplateOrigin, WorkflowTemplate};

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
            "Creates a marked PDF copy of a supported Paddle invoice page. Loads its stylesheets and scripts from my.paddle.com, static.paddle.com and cdn.paddle.com as data only: stylesheets are checked on every run and their layout, like the scripts, must match the tested version; scripts are never executed. No login.",
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
        hosts: vec!["my.paddle.com".into(), "static.paddle.com".into(), "cdn.paddle.com".into()],
        steps: vec![
            Step::MailLink { host: "my.paddle.com".into(), label: None },
            Step::Get,
            Step::RenderInvoiceCopy { profile: InvoiceProfile::PaddleSetapp, version: 2 },
            Step::VerifyPdf,
            Step::SaveToTarget,
        ],
        ..direct.clone()
    };
    paddle.binding.typesetter = 1;
    paddle.binding.fonts_digest = invoice::binding(2);
    paddle.binding.resources = paddle_scripts();
    paddle.binding.layout = Some(PADDLE_LAYOUT.into());
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

/// The layout fingerprint of the real Paddle/Setapp invoice pages (August to November 2026), identical on all of
/// them. A draft test adopts a newer one; a saved workflow stops with `provider_changed`.
const PADDLE_LAYOUT: &str = "a0b64d3f30a791c12a2f946ee4f7014ee67e9deb665037b36b9048e21ae80e50";

/// The provider scripts witnessed on the real Paddle/Setapp invoice pages (August to November 2026): jQuery and
/// jQuery UI from the CDN, select2 and the invoice script, and the inline address-form handler. A draft test
/// adopts newer ones; a saved workflow stops with `provider_changed`.
fn paddle_scripts() -> Vec<ResourcePin> {
    [
        ("my.paddle.com", "/", "adb4739d687831bbc5b85bf18e800ac6047198900efe16c17f2360781b49692c", true),
        (
            "cdn.paddle.com",
            "/external/jquery-3.6.0.min.js",
            "ff1523fb7389539c84c65aba19260648793bb4f5e29329d2ee8804bc37a3fe6e",
            false,
        ),
        (
            "cdn.paddle.com",
            "/external/jquery-ui-1.12.1.min.js",
            "55accff7b642c2d7a402cbe03c1494c0f14a76bc03dee9d47d219562b6a152a5",
            false,
        ),
        (
            "static.paddle.com",
            "/assets/libraries/js/select2.min.js",
            "1cd91b9dd3e258ce4421181a85cde15a2b860fa0adc0580c7c4534a37cde69ba",
            false,
        ),
        (
            "static.paddle.com",
            "/assets/js/customer/invoice.js",
            "324072216721de7328ce00b3611c149e0588443555b510494d89ecb4cd33d3f5",
            false,
        ),
    ]
    .into_iter()
    .map(|(host, path, sha256, inline)| ResourcePin {
        host: host.into(),
        path: path.into(),
        sha256: sha256.into(),
        inline,
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
