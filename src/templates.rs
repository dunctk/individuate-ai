use crate::agent::{
    AdminUserAccess, ChatLog, RelationshipProfile, Session, SocialGraph, User, TTS_VOICES,
};
use crate::cycle::{BodyOnboardingPreference, CycleDashboard, CycleProfile};
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
    env.add_template("subscribe", include_str!("../templates/subscribe.html"))
        .unwrap();
    env.add_template("admin", include_str!("../templates/admin.html"))
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
    env.add_template(
        "social_graph",
        include_str!("../templates/social_graph.html"),
    )
    .unwrap();
    env.add_template("timeline", include_str!("../templates/timeline.html"))
        .unwrap();
    env.add_template(
        "inner_work_timeline",
        include_str!("../templates/inner_work_timeline.html"),
    )
    .unwrap();
    env.add_template("import", include_str!("../templates/import.html"))
        .unwrap();
    env.add_template("cycle", include_str!("../templates/cycle.html"))
        .unwrap();
    env
}

pub fn render_login(env: &Environment) -> String {
    env.get_template("login")
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

pub fn render_subscribe(env: &Environment) -> String {
    env.get_template("subscribe")
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
    cycle: &CycleDashboard,
    body_onboarding: &BodyOnboardingPreference,
) -> String {
    env.get_template("home")
        .unwrap()
        .render(json!({
            "user_id": user.id,
            "username": user.username,
            "session_id": session_id,
            "messages": chat_message_values(messages),
            "cycle_enabled": cycle.profile.enabled && !cycle.profile.paused,
            "cycle_show_in_chat": cycle.profile.show_in_chat,
            "cycle_day": cycle.prediction.cycle_day,
            "show_body_onboarding": !body_onboarding.completed && !cycle.profile.enabled,
            "body_identity": body_onboarding.identity.as_deref().unwrap_or(""),
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
            "is_admin": crate::billing::is_admin_email(&user.username),
        }))
        .unwrap()
}

pub fn render_admin(env: &Environment, users: &[AdminUserAccess], admin_email: &str) -> String {
    let users = users
        .iter()
        .map(|user| {
            let is_admin = user.username.eq_ignore_ascii_case(admin_email);
            let has_paid_access = user
                .billing_status
                .as_deref()
                .is_some_and(|status| matches!(status, "active" | "trialing" | "past_due"));
            json!({
                "id": user.id,
                "username": user.username,
                "billing_status": user.billing_status,
                "has_paid_access": has_paid_access,
                "has_lifetime_access": user.has_lifetime_access,
                "is_admin": is_admin,
            })
        })
        .collect::<Vec<_>>();
    env.get_template("admin")
        .unwrap()
        .render(json!({
            "users": users,
            "admin_email": admin_email,
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
    cycle_profile: &CycleProfile,
    body_onboarding: &BodyOnboardingPreference,
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
            "cycle_enabled": cycle_profile.enabled,
            "cycle_paused": cycle_profile.paused,
            "cycle_ai_context_enabled": cycle_profile.ai_context_enabled,
            "body_onboarding_completed": body_onboarding.completed,
            "body_identity_label": body_onboarding.identity_label(),
        }))
        .unwrap()
}

pub fn render_cycle(env: &Environment, dashboard: &CycleDashboard) -> String {
    let dashboard_json = serde_json::to_string(dashboard)
        .unwrap_or_else(|_| "{}".to_string())
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    env.get_template("cycle")
        .unwrap()
        .render(json!({
            "dashboard": dashboard,
            "dashboard_json": dashboard_json,
            "enabled": dashboard.profile.enabled,
            "paused": dashboard.profile.paused,
            "cycle_day": dashboard.prediction.cycle_day,
            "state_label": dashboard.prediction.state_label,
            "state_detail": dashboard.prediction.state_detail,
            "confidence": dashboard.prediction.confidence,
            "next_start": dashboard.prediction.next_start,
            "next_start_earliest": dashboard.prediction.next_start_earliest,
            "next_start_latest": dashboard.prediction.next_start_latest,
            "tracking_mode": dashboard.profile.tracking_mode,
            "typical_cycle_days": dashboard.profile.typical_cycle_days,
            "ai_context_enabled": dashboard.profile.ai_context_enabled,
            "show_in_chat": dashboard.profile.show_in_chat,
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

pub fn render_timeline(env: &Environment) -> String {
    env.get_template("timeline")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

pub fn render_inner_work_timeline(env: &Environment) -> String {
    env.get_template("inner_work_timeline")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

pub fn render_import(env: &Environment) -> String {
    env.get_template("import")
        .unwrap()
        .render(json!({}))
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
            "login",
            "recovery",
            "signup",
            "subscribe",
            "home",
            "sidebar",
            "chat_messages",
            "profile_drawer",
            "social_graph",
            "timeline",
            "inner_work_timeline",
            "import",
            "privacy_security",
            "cycle",
        ] {
            env.get_template(name).expect("template registered");
        }
    }

    #[test]
    fn social_graph_projection_is_presented_as_the_mind_map() {
        let html = render_social_graph(&create_env(), &SocialGraph::default(), "user-1");

        assert!(html.contains(">Mind Map</h1>"));
        assert!(!html.contains(">Social Graph</h1>"));
        assert!(html.contains("fetch('/api/mind-map')"));
        assert!(html.contains("Find a person or memory"));
        assert!(html.contains("href=\"/timeline\""));
        assert!(html.contains("expandedSocialGroups"));
        assert!(html.contains("mind-map-inspector"));
    }

    #[test]
    fn timeline_page_contains_private_controls() {
        let html = render_timeline(&create_env());
        assert!(html.contains("/api/timeline"));
        assert!(html.contains("Why is this here?"));
        assert!(html.contains("Pin event"));
    }

    #[test]
    fn inner_work_timeline_is_opt_in_and_time_scoped() {
        let html = render_inner_work_timeline(&create_env());
        assert!(html.contains("/api/inner-work-timeline"));
        assert!(html.contains("All time"));
        assert!(html.contains("Generate timeline"));
        assert!(html.contains("your own reflection messages"));
    }

    #[test]
    fn import_page_explains_user_only_memory_extraction() {
        let html = render_import(&create_env());
        assert!(html.contains("only your own messages are sent to memory extraction"));
        assert!(html.contains("/api/import/gemini"));
    }

    #[test]
    fn base_template_contains_ios_pwa_metadata() {
        let html = create_env()
            .get_template("login")
            .unwrap()
            .render(minijinja::context! {})
            .unwrap();
        assert!(html.contains("/manifest.webmanifest"));
        assert!(html.contains("apple-mobile-web-app-capable"));
        assert!(html.contains("individuateai.css?v=20260721-inner-work"));
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
        assert!(html.contains("Read therapist message aloud"));
        assert!(html.contains("therapist-audio-playback"));
        assert!(html.contains("avatar-orb"));
        assert!(html.contains("role-label"));
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
        assert!(html.contains("Step 1 of 2"));
        assert!(html.contains("before choosing and paying for a plan"));
    }

    #[test]
    fn subscription_page_has_paid_only_usd_and_eur_options() {
        let env = create_env();
        let html = render_subscribe(&env);
        assert!(html.contains("$24.99 monthly"));
        assert!(html.contains("$239 yearly"));
        assert!(html.contains("€29.99 monthly"));
        assert!(html.contains("€289 yearly"));
        assert!(html.contains("/api/billing/checkout"));
        assert!(html.contains("Step 2 of 2"));
        assert!(html.contains("Account and passkey created"));
        assert!(html.contains("data-currency-panel=\"usd\""));
        assert!(html.contains("data-currency-panel=\"eur\""));
        assert!(html.contains("Show euro pricing"));
        assert!(html.contains("billing_currency"));
        assert!(!html.to_ascii_lowercase().contains("unlimited"));
        assert!(!html.to_ascii_lowercase().contains("free trial"));
    }

    #[test]
    fn admin_page_renders_access_controls() {
        let env = create_env();
        let users = vec![AdminUserAccess {
            id: "user-1".into(),
            username: "person@example.com".into(),
            billing_status: None,
            has_lifetime_access: false,
        }];
        let html = render_admin(&env, &users, "admin@example.com");
        assert!(html.contains("Complimentary access"));
        assert!(html.contains("person@example.com"));
        assert!(html.contains("Grant lifetime"));
        assert!(html.contains("/api/admin/users/"));
        assert!(html.contains("does not cancel an existing paid subscription"));
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
        let html = render_home(
            &env,
            &user,
            "",
            &[],
            &crate::cycle::build_dashboard(
                CycleProfile::default(),
                Vec::new(),
                Vec::new(),
                crate::cycle::today_utc(),
            ),
            &BodyOnboardingPreference::default(),
        );
        assert!(html.contains("app-shell"));
        assert!(html.contains("app-viewport"));
        assert!(html.contains("chat-stage"));
        assert!(html.contains("composer-shell"));
        assert!(html.contains("window.visualViewport"));
        assert!(html.contains("viewport?.offsetTop"));
        assert!(html.contains("--chat-viewport-offset-top"));
        assert!(html.contains("min-w-0 w-full items-end"));
        assert!(html.contains("brand-mark hidden sm:inline"));
        assert!(html.contains("href=\"/mind-map\""));
        assert!(html.contains("href=\"/timeline\""));
        assert!(html.contains("id=\"focus-drawer\""));
        assert!(html.contains("openFocusDrawer()"));
        assert!(html.contains("/api/core-patterns"));
        assert!(html.contains("Nothing guides future chats until you activate it."));
        assert!(!html.contains("href=\"/social-graph\""));
        assert!(html.contains("copy-thread-button"));
        assert!(html.contains("<span class=\"thread-copy-label\">Copy</span>"));
        assert!(!html.contains("thread-copy-label sr-only"));
        assert!(html.contains("copyThreadMarkdown()"));
        assert!(html.contains("# Conversation"));
        assert!(html.contains("voice-live"));
        assert!(html.contains("Deepgram live"));
        assert!(html.contains("playDeepgramListeningTone()"));
        assert!(html.contains("auto-read-responses"));
        assert!(html.contains("therapist-audio-trigger"));
        assert!(html.contains("readTherapistMessage(this)"));
        assert!(html.contains("therapist-audio-play-icon"));
        assert!(html.contains("seekSpokenResponse(-15)"));
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
        assert!(html.contains("How do you describe yourself?"));
        assert!(html.contains("Would you like to track your periods?"));
        assert!(html.contains("/api/onboarding/body"));
    }

    #[test]
    fn profile_drawer_renders_voice_settings_and_selected_voice() {
        let env = create_env();
        let html = render_profile_drawer(
            &env,
            &[],
            "",
            "aura-2-helena-en",
            &CycleProfile::default(),
            &BodyOnboardingPreference::default(),
        );
        assert!(html.contains(">Settings</h2>"));
        assert!(html.contains("Import Gemini chats"));
        assert!(!html.contains("Relationship profiles"));
        assert!(html.contains("Response voice"));
        assert!(html.contains("data-voice=\"aura-2-helena-en\""));
        assert!(html.contains("data-selected=\"true\""));
        assert!(html.contains("Preview Helena"));
        assert!(html.contains("/api/settings/voice"));
        assert!(html.contains("About you"));
    }

    #[test]
    fn cycle_page_renders_opt_in_and_tracking_states() {
        let env = create_env();
        let disabled = crate::cycle::build_dashboard(
            CycleProfile::default(),
            Vec::new(),
            Vec::new(),
            crate::cycle::parse_date("2026-07-17").unwrap(),
        );
        let setup_html = render_cycle(&env, &disabled);
        assert!(setup_html.contains("Enable private tracking"));
        assert!(setup_html.contains("not used for fertility"));

        let enabled = crate::cycle::build_dashboard(
            CycleProfile {
                enabled: true,
                ..CycleProfile::default()
            },
            vec![crate::cycle::CycleEvent {
                id: "event-1".to_string(),
                kind: "bleeding_started".to_string(),
                local_date: "2026-07-17".to_string(),
                source: "manual".to_string(),
                ..crate::cycle::CycleEvent::default()
            }],
            Vec::new(),
            crate::cycle::parse_date("2026-07-17").unwrap(),
        );
        let dashboard_html = render_cycle(&env, &enabled);
        assert!(dashboard_html.contains("Recorded bleeding window"));
        assert!(dashboard_html.contains("cycleDashboard"));
        assert!(dashboard_html.contains("Delete all cycle data"));
    }
}
