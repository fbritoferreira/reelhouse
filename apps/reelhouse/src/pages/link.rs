use dioxus::prelude::*;

use crate::session::client;
use crate::widgets::describe;

/// Approves a code shown on a TV. Opened by scanning the TV's QR code, which fills in `code`.
#[component]
pub fn LinkDevice(code: String) -> Element {
    let mut entered = use_signal(|| code.clone());
    let mut status = use_signal(|| None::<(bool, String)>);
    let submit = move |e: FormEvent| async move {
        e.prevent_default();
        match client().device_approve(&entered()).await {
            Ok(()) => status.set(Some((true, "Linked. The device will sign in within a few seconds.".into()))),
            Err(reelhouse_core::ClientError::Status(404, _)) => {
                status.set(Some((false, "That code doesn't match a device waiting to sign in. Check the code on the TV, or start again there.".into())))
            }
            Err(e) => status.set(Some((false, describe(&e)))),
        }
    };
    rsx! {
        div { class: "admin",
            section {
                h2 { "Link a device" }
                p { class: "quiet", "Enter the code shown on your TV to sign it in as you." }
                form { class: "inline-form", onsubmit: submit,
                    input {
                        autofocus: true,
                        class: "code-input",
                        placeholder: "ABCD-2345",
                        autocomplete: "off",
                        value: "{entered}",
                        oninput: move |e| entered.set(e.value()),
                    }
                    button { class: "primary", r#type: "submit", "Link device" }
                }
                if let Some((ok, msg)) = status() {
                    p { class: if ok { "linked" } else { "notice" }, "{msg}" }
                }
            }
        }
    }
}
