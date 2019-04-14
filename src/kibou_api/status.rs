use activity::get_activity_by_id;
use activitypub::activity::Tag;
use activitypub::controller::activity_create;
use activitypub::controller::note;
use actor::get_actor_by_acct;
use actor::get_actor_by_uri;
use actor::Actor;
use database;
use diesel::PgConnection;
use html;
use regex::Regex;
use web_handler::federator;

pub fn build(actor: String, mut content: String, visibility: &str, in_reply_to: Option<String>) {
    let database = database::establish_connection();
    let serialized_actor: Actor = get_actor_by_uri(&database, &actor).unwrap();

    let mut direct_receipients: Vec<String> = vec![];
    let mut receipients: Vec<String> = vec![];
    let mut inboxes: Vec<String> = vec![];
    let mut tags: Vec<serde_json::Value> = vec![];

    let parsed_mentions = parse_mentions(html::strip_tags(content));
    direct_receipients.extend(parsed_mentions.0);
    inboxes.extend(parsed_mentions.1);
    tags.extend(parsed_mentions.2);
    content = parsed_mentions.3;

    match visibility {
        "public" => {
            direct_receipients.push("https://www.w3.org/ns/activitystreams#Public".to_string());
            receipients.push(format!("{}/followers", actor));
            inboxes.extend(handle_follower_inboxes(
                &database,
                &serialized_actor.followers,
            ));
        }

        "unlisted" => {
            direct_receipients.push(format!("{}/followers", actor));
            receipients.push("https://www.w3.org/ns/activitystreams#Public".to_string());
            inboxes.extend(handle_follower_inboxes(
                &database,
                &serialized_actor.followers,
            ));
        }

        "private" => {
            direct_receipients.push(format!("{}/followers", actor));
            inboxes.extend(handle_follower_inboxes(
                &database,
                &serialized_actor.followers,
            ));
        }

        _ => (),
    }

    inboxes.dedup();

    let activitypub_note = note(
        &actor,
        handle_in_reply_to(in_reply_to),
        content,
        direct_receipients.clone(),
        receipients.clone(),
        tags,
    );
    let activitypub_activity_create = activity_create(
        &actor,
        serde_json::to_value(activitypub_note).unwrap(),
        direct_receipients,
        receipients,
    );
    federator::enqueue(
        serialized_actor,
        serde_json::json!(&activitypub_activity_create),
        inboxes,
    );
}

fn handle_follower_inboxes(
    db_connection: &PgConnection,
    followers: &serde_json::Value,
) -> Vec<String> {
    let ap_followers = serde_json::from_value(followers["activitypub"].clone());
    let mut follow_data: Vec<serde_json::Value> = ap_followers.unwrap();
    let mut inboxes: Vec<String> = vec![];

    for follower in follow_data {
        match get_actor_by_uri(db_connection, follower["href"].as_str().unwrap()) {
            Ok(actor) => {
                if !actor.local {
                    inboxes.push(actor.inbox.unwrap());
                }
            }
            Err(_) => (),
        }
    }
    return inboxes;
}

fn handle_in_reply_to(local_id: Option<String>) -> Option<String> {
    let database = database::establish_connection();

    if local_id.is_some() {
        match get_activity_by_id(&database, local_id.unwrap().parse::<i64>().unwrap()) {
            Ok(activity) => Some(activity.data["object"]["id"].as_str().unwrap().to_string()),
            Err(_) => None,
        }
    } else {
        return None;
    }
}

fn parse_mentions(content: String) -> (Vec<String>, Vec<String>, Vec<serde_json::Value>, String) {
    let acct_regex = Regex::new(r"@[a-zA-Z0-9._-]+@[a-zA-Z0-9._-]+\.[a-zA-Z0-9_-]+\w").unwrap();
    let database = database::establish_connection();

    let mut receipients: Vec<String> = vec![];
    let mut inboxes: Vec<String> = vec![];
    let mut new_content: String = content.clone();
    let mut tags: Vec<serde_json::Value> = vec![];

    for mention in acct_regex.captures_iter(&content) {
        match get_actor_by_acct(
            &database,
            mention.get(0).unwrap().as_str().to_string().split_off(1),
        ) {
            Ok(actor) => {
                let tag: Tag = Tag {
                    _type: String::from("Mention"),
                    href: actor.actor_uri.clone(),
                    name: mention.get(0).unwrap().as_str().to_string(),
                };

                if !actor.local {
                    inboxes.push(actor.inbox.unwrap());
                }
                receipients.push(actor.actor_uri.clone());
                tags.push(serde_json::to_value(tag).unwrap());
                new_content = str::replace(
                    &new_content,
                    mention.get(0).unwrap().as_str(),
                    &format!(
                        "<a class=\"mention\" href=\"{uri}\">{acct}</a>",
                        uri = actor.actor_uri,
                        acct = mention.get(0).unwrap().as_str()
                    ),
                );
            }
            Err(_) => (),
        }
    }
    (receipients, inboxes, tags, new_content)
}
