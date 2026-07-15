use crate::agent::{ChatLog, RelationshipProfile, Session, SocialGraph, User, TTS_VOICES};
use minijinja::Environment;
use serde_json::json;

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_inline_markdown(value: &str) -> String {
    let mut html = String::new();
    let mut rest = value;
    let mut strong_open = false;

    while let Some(index) = rest.find("**") {
        html.push_str(&escape_html(&rest[..index]));
        html.push_str(if strong_open { "</strong>" } else { "<strong>" });
        strong_open = !strong_open;
        rest = &rest[index + 2..];
    }

    html.push_str(&escape_html(rest));
    if strong_open {
        html.push_str("</strong>");
    }
    html
}

fn render_chat_markdown(value: &str) -> String {
    value
        .trim()
        .split("\n\n")
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            format!(
                "<p>{}</p>",
                render_inline_markdown(part.trim()).replace('\n', "<br>")
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn create_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.add_template("base.html", include_str!("../templates/base.html"))
        .unwrap();
    env.add_template("landing", include_str!("../templates/landing.html"))
        .unwrap();
    env.add_template(
        "privacy_security",
        include_str!("../templates/privacy_security.html"),
    )
    .unwrap();
    env.add_template("login", include_str!("../templates/login.html"))
        .unwrap();
    env.add_template("recovery", include_str!("../templates/recovery.html"))
        .unwrap();
    env.add_template("signup", include_str!("../templates/signup.html"))
        .unwrap();
    env.add_template("home", include_str!("../templates/home.html"))
        .unwrap();
    env.add_template("sidebar", include_str!("../templates/sidebar.html"))
        .unwrap();
    env.add_template(
        "chat_messages",
        include_str!("../templates/chat_messages.html"),
    )
    .unwrap();
    env.add_template(
        "profile_drawer",
        include_str!("../templates/profile_drawer.html"),
    )
    .unwrap();
    env.add_template("mind_map", include_str!("../templates/mind_map.html"))
        .unwrap();
    env.add_template(
        "social_graph",
        include_str!("../templates/social_graph.html"),
    )
    .unwrap();
    env
}

pub fn render_login(env: &Environment) -> String {
    env.get_template("login")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

pub fn render_landing(env: &Environment) -> String {
    env.get_template("landing")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

pub fn render_privacy_security(env: &Environment) -> String {
    env.get_template("privacy_security")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

pub fn render_recovery(env: &Environment) -> String {
    env.get_template("recovery")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

pub fn render_signup(env: &Environment) -> String {
    env.get_template("signup")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

fn chat_message_values(messages: &[ChatLog]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| {
            json!({
                "role": m.role,
                "content": m.content,
                "content_html": render_chat_markdown(&m.content),
            })
        })
        .collect()
}

pub fn render_home(
    env: &Environment,
    user: &User,
    session_id: &str,
    messages: &[ChatLog],
) -> String {
    env.get_template("home")
        .unwrap()
        .render(json!({
            "user_id": user.id,
            "username": user.username,
            "session_id": session_id,
            "messages": chat_message_values(messages),
        }))
        .unwrap()
}

pub fn render_sidebar(env: &Environment, sessions: &[Session], user: &User) -> String {
    let session_list: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "title": s.title,
                "preview": s.preview,
                "date": s.date,
            })
        })
        .collect();
    env.get_template("sidebar")
        .unwrap()
        .render(json!({
            "sessions": session_list,
            "username": user.username,
        }))
        .unwrap()
}

pub fn render_chat_messages(env: &Environment, messages: &[ChatLog]) -> String {
    let msg_list = chat_message_values(messages);
    env.get_template("chat_messages")
        .unwrap()
        .render(json!({
            "messages": msg_list,
        }))
        .unwrap()
}

pub fn render_profile_drawer(
    env: &Environment,
    profiles: &[RelationshipProfile],
    slug: &str,
    selected_voice: &str,
) -> String {
    let selected = profiles.iter().find(|p| p.slug == slug);
    let profile_list: Vec<serde_json::Value> = profiles
        .iter()
        .map(|p| {
            json!({
                "slug": p.slug,
                "display_name": p.display_name,
            })
        })
        .collect();
    let voice_list: Vec<serde_json::Value> = TTS_VOICES
        .iter()
        .map(|voice| {
            json!({
                "id": voice.id,
                "name": voice.name,
                "description": voice.description,
                "sample_url": voice.sample_url,
            })
        })
        .collect();
    env.get_template("profile_drawer")
        .unwrap()
        .render(json!({
            "profiles": profile_list,
            "voices": voice_list,
            "selected_voice": selected_voice,
            "selected_slug": slug,
            "display_name": selected.map_or("", |p| &p.display_name),
            "relationship_type": selected.map_or("", |p| &p.relationship_type),
            "background": selected.map_or("", |p| &p.background),
            "goals": selected.map_or(&Vec::<String>::new(), |p| &p.goals),
            "triggers": selected.map_or(&Vec::<String>::new(), |p| &p.triggers),
            "do_not_say": selected.map_or(&Vec::<String>::new(), |p| &p.do_not_say),
            "effective_tone": selected.map_or(&Vec::<String>::new(), |p| &p.effective_tone),
            "recent_events": selected.map_or(&Vec::<String>::new(), |p| &p.recent_events),
            "boundaries": selected.map_or(&Vec::<String>::new(), |p| &p.boundaries),
        }))
        .unwrap()
}

pub fn render_mind_map(env: &Environment, graph: &serde_json::Value, user_id: &str) -> String {
    let graph_json = graph.clone();
    env.get_template("mind_map")
        .unwrap()
        .render(json!({
            "graph_data": graph_json,
            "user_id": user_id,
        }))
        .unwrap()
}

pub fn render_social_graph(env: &Environment, graph: &SocialGraph, user_id: &str) -> String {
    let graph_json = serde_json::to_value(graph).unwrap_or(json!({"nodes":[],"edges":[]}));
    env.get_template("social_graph")
        .unwrap()
        .render(json!({
            "graph_data": graph_json,
            "user_id": user_id,
        }))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_templates_compile() {
        let env = create_env();
        for name in [
            "base.html",
            "landing",
            "login",
            "recovery",
            "signup",
            "home",
            "sidebar",
            "chat_messages",
            "profile_drawer",
            "mind_map",
            "social_graph",
            "privacy_security",
        ] {
            env.get_template(name).expect("template registered");
        }
    }

    #[test]
    fn base_template_contains_ios_pwa_metadata() {
        let html = create_env()
            .get_template("landing")
            .unwrap()
            .render(minijinja::context! {})
            .unwrap();
        assert!(html.contains("/manifest.webmanifest"));
        assert!(html.contains("apple-mobile-web-app-capable"));
        assert!(html.contains("individuateai.css?v=20260715-chat-copy"));
        assert!(html.contains("apple-touch-icon.png"));
        assert!(html.contains("navigator.serviceWorker.register('/service-worker.js'"));
    }

    #[test]
    fn renders_chat_messages_bubbles() {
        let env = create_env();
        let messages = vec![
            ChatLog {
                role: "user".into(),
                content: "hello".into(),
            },
            ChatLog {
                role: "assistant".into(),
                content: "hi there".into(),
            },
        ];
        let html = render_chat_messages(&env, &messages);
        assert!(html.contains("bubble-user"));
        assert!(html.contains("bubble-therapist"));
        assert!(html.contains("avatar-orb"));
        assert!(html.contains("role-label"));
    }

    #[test]
    fn landing_page_contains_product_and_trust_sections() {
        let env = create_env();
        let html = render_landing(&env);
        assert!(html.contains("Open-Source AI Therapist App | IndividuateAI"));
        assert!(html.contains("An open-source AI therapist app that helps you see your patterns"));
        assert!(html.contains("name=\"description\""));
        assert!(html.contains("not a substitute for a licensed mental health professional"));
        assert!(html.contains("Illustrative mind map"));
        assert_eq!(html.matches("dy=\".35em\"").count(), 16);
        assert!(html.contains("Illustrative social graph"));
        assert!(html.contains("Privacy &amp; security"));
        assert!(html.contains("Check the code yourself"));
        assert!(html.contains("If you lose every passkey and your recovery key"));
        assert!(html.contains("The AI provider must read a message to answer it"));
        assert!(html.contains("github.com/dunctk/individuate-ai"));
        assert!(html.contains("href=\"/login\""));
        assert!(html.contains("href=\"/privacy-and-security\""));
    }

    #[test]
    fn privacy_security_page_explains_boundaries_and_recovery() {
        let env = create_env();
        let html = render_privacy_security(&env);
        assert!(html.contains("Your inner life should not become someone else"));
        assert!(html.contains("Where encryption cannot help"));
        assert!(html.contains("There is deliberately no password-reset back door"));
        assert!(html.contains("Zero-retention AI routing"));
        assert!(html.contains("processed by Deepgram within the European Union"));
        assert!(html.contains("retained only for the time required to process the request"));
        assert!(html.contains("privacy-vault"));
        assert!(html.contains("privacy-mobile-toc"));
        assert!(html.contains("privacy-boundary"));
        assert!(html.contains("privacy-access-flow"));
        assert!(html.contains("Security dossier"));
        assert!(html.contains("href=\"/\""));
    }

    #[test]
    fn login_uses_account_selecting_passkey_flow() {
        let env = create_env();
        let html = render_login(&env);
        assert!(html.contains("Sign in with Passkey"));
        assert!(!html.contains("passkey-email"));
        assert!(!html.contains("recovery-login-form"));
        assert!(html.contains("href=\"/recovery\""));
        assert!(html.contains("/api/passkey/login/start"));
    }

    #[test]
    fn recovery_page_owns_recovery_login_form() {
        let env = create_env();
        let html = render_recovery(&env);
        assert!(html.contains("recovery-login-form"));
        assert!(html.contains("/api/recovery/login"));
        assert!(html.contains("href=\"/login\""));
    }

    #[test]
    fn signup_explains_irrecoverable_account_loss() {
        let env = create_env();
        let html = render_signup(&env);
        assert!(html.contains("Your account cannot be reset"));
        assert!(html.contains("We cannot recover them for you"));
        assert!(html.contains("recovery-warning-ack"));
        assert!(html.contains("must save my recovery key"));
    }

    #[test]
    fn renders_chat_markdown_safely() {
        let env = create_env();
        let messages = vec![ChatLog {
            role: "assistant".into(),
            content: "**Pattern:** <script>alert(1)</script>\n\nNext line".into(),
        }];
        let html = render_chat_messages(&env, &messages);
        assert!(html.contains("<strong>Pattern:</strong>"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("<p>Next line</p>"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn renders_home_shell() {
        let env = create_env();
        let user = User {
            id: "u1".into(),
            username: "Dunc".into(),
        };
        let html = render_home(&env, &user, "", &[]);
        assert!(html.contains("app-shell"));
        assert!(html.contains("app-viewport"));
        assert!(html.contains("chat-stage"));
        assert!(html.contains("composer-shell"));
        assert!(html.contains("window.visualViewport"));
        assert!(html.contains("viewport?.offsetTop"));
        assert!(html.contains("--chat-viewport-offset-top"));
        assert!(html.contains("min-w-0 w-full items-end"));
        assert!(html.contains("brand-mark hidden sm:inline"));
        assert!(html.contains("copy-thread-button"));
        assert!(html.contains("<span class=\"thread-copy-label\">Copy</span>"));
        assert!(!html.contains("thread-copy-label sr-only"));
        assert!(html.contains("copyThreadMarkdown()"));
        assert!(html.contains("# Conversation"));
        assert!(html.contains("voice-live"));
        assert!(html.contains("Deepgram live"));
        assert!(html.contains("playDeepgramListeningTone()"));
        assert!(html.contains("auto-read-responses"));
        assert!(html.contains("response-audio-play"));
        assert!(html.contains("seekSpokenResponse(-15)"));
        assert!(html.contains("replaySpokenResponse()"));
        assert!(html.contains("wss://api.eu.deepgram.com/v1/speak"));
        assert!(html.contains("encoding: 'linear16'"));
        assert!(html.contains("type: 'Flush'"));
        assert!(html.contains("/api/speak"));
        assert!(html.contains("/api/deepgram/token"));
        assert!(html.contains("wss://api.eu.deepgram.com/v1/listen"));
        assert!(html.contains("mip_opt_out: 'true'"));
        assert!(!html.contains("wss://api.deepgram.com/v1/listen"));
        assert!(!html.contains("wss://api.deepgram.com/v1/speak"));
        assert!(!html.contains("window.webkitSpeechRecognition"));
        assert!(html.contains("seed-orb"));
        assert!(html.contains("M12 19V5"));
        assert!(html.contains("mandala-avatar.jpg"));
        assert!(html.contains("Create a new recovery key"));
        assert!(html.contains("Your current recovery key will keep working"));
        assert!(html.contains("I saved this recovery key somewhere safe"));
    }

    #[test]
    fn profile_drawer_renders_voice_settings_and_selected_voice() {
        let env = create_env();
        let html = render_profile_drawer(&env, &[], "", "aura-2-helena-en");
        assert!(html.contains("Profiles &amp; settings"));
        assert!(html.contains("Response voice"));
        assert!(html.contains("data-voice=\"aura-2-helena-en\""));
        assert!(html.contains("data-selected=\"true\""));
        assert!(html.contains("Preview Helena"));
        assert!(html.contains("/api/settings/voice"));
    }
}
