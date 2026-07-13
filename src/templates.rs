use crate::agent::{ChatLog, RelationshipProfile, Session, SocialGraph, User};
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
    env.add_template("login", include_str!("../templates/login.html"))
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
    env.add_template(
        "forgot_password",
        include_str!("../templates/forgot_password.html"),
    )
    .unwrap();
    env.add_template(
        "reset_password",
        include_str!("../templates/reset_password.html"),
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
    env.get_template("profile_drawer")
        .unwrap()
        .render(json!({
            "profiles": profile_list,
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

pub fn render_forgot_password(env: &Environment) -> String {
    env.get_template("forgot_password")
        .unwrap()
        .render(json!({}))
        .unwrap()
}

pub fn render_reset_password(env: &Environment, token: &str) -> String {
    env.get_template("reset_password")
        .unwrap()
        .render(json!({
            "token": token,
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
            "login",
            "signup",
            "home",
            "sidebar",
            "chat_messages",
            "profile_drawer",
            "mind_map",
            "social_graph",
            "forgot_password",
            "reset_password",
        ] {
            env.get_template(name).expect("template registered");
        }
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
        assert!(html.contains("chat-stage"));
        assert!(html.contains("seed-orb"));
        assert!(html.contains("mandala-avatar.jpg"));
    }
}
