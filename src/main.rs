use rand::Rng;
use serde_json::json;
use serenity::{
    framework::StandardFramework,
    http,
    model::{channel::Message, gateway::Ready, id::ChannelId, webhook::Webhook},
    prelude::*,
};

use log::{debug, error, info, trace, warn};
use std::{fs, path::PathBuf};

use markov::Chain;

mod chains;
use chains::*;

mod commands;
use commands::ADMIN_GROUP;

mod config;

use config::Config;

fn webhook(ctx: &Context, cid: ChannelId, name: String) -> Result<Webhook, serenity::Error> {
    ctx.http
        .create_webhook(*cid.as_u64(), &json!({ "name": name }))
}

struct Handler;

impl EventHandler for Handler {
    fn ready(&self, ctx: Context, ready: Ready) {
        info!("Connected to discord");
    }

    fn message(&self, mut ctx: Context, msg: Message) {
        if msg.webhook_id.is_some() {
            return;
        }

        if let Some(gid) = msg.guild_id {
            let author_id = msg.author.id;
            ctx.data
                .write()
                .get_mut::<UserChains>()
                .unwrap()
                .feed(&author_id, &msg.content);
            if let Some(guild_id) = msg.guild_id {
                trace!(
                    "Recieved message in guild {} from author {}",
                    guild_id,
                    author_id
                );
                if msg.mentions.len() > 0 || msg.mention_roles.len() > 0 {
                    let mentions: Vec<_> = ctx
                        .data
                        .read()
                        .get::<UserChains>()
                        .expect("No chains loaded")
                        .user_ids()
                        .iter()
                        .map(|id| id.to_user(&ctx).expect("couldn't retrieve user"))
                        .filter(|user| {
                            msg.mentions.contains(&user)
                                || msg
                                    .mention_roles
                                    .iter()
                                    .any(|role| user.has_role(&ctx, guild_id, role).is_ok())
                        })
                        .collect();

                    trace!("Message contains {} unique mentions", mentions.len());

                    if let Ok(hook) = webhook(&mut ctx, msg.channel_id, "wide hook".to_owned()) {
                        let mut rng = rand::thread_rng();

                        for user in mentions {
                            let name = user.nick_in(&ctx, gid).unwrap_or(user.name.clone());
                            let a_url = user
                                .avatar_url()
                                .unwrap_or("https://crates.io/assets/Cargo-Logo-Small-c39abeb466d747f3be442698662c5260.png".to_string());
                            ctx.data
                                .read()
                                .get::<UserChains>()
                                .unwrap()
                                .message_iter(&user.id, rng.gen_range(1, 5))
                                .unwrap()
                                .for_each(|res| {
                                    if let Err(why) = hook.execute(&ctx, false, |w| {
                                        w.username(&name).avatar_url(&a_url).content(&res)
                                    }) {
                                        warn!(
                                            "Could not send message \"{}: {}\" -- {}",
                                            &name, &res, why
                                        );
                                    }
                                });
                        }

                        if let Err(why) = hook.delete(ctx) {
                            warn!("Could not delete webhook: {}", why);
                        }
                    } else {
                        warn!("Could not create webhook");
                    }
                }
            }
        }
    }
}

fn main() {
    config::setup_logger();

    let config: Config =
        toml::from_str(&fs::read_to_string("Bizarro.toml").expect("Didn't find Bizarro.toml"))
            .expect("Invalid Bizarro.toml");
    info!("Config loaded from toml");

    let mut client = match Client::new(&config.discord_token, Handler) {
        Ok(client) => client,
        Err(why) => {
            error!("Error creating client: {}", why);
            std::process::exit(69); // Service unavailable exit code.
        }
    };

    client.with_framework(
        StandardFramework::new()
            .configure(|c| c.prefix(&config.prefix))
            .group(&ADMIN_GROUP)
            // .cmd("ping", commands::ping)
            // .cmd("regen", commands::regenerate)
            // .cmd("save", commands::save),
    );
    info!("Client created");

    let chains = UserChains::load(&config.chain_storage_dir).expect("couldn't load chains");

    {
        let mut data = client.data.write();
        data.insert::<UserChains>(chains);
        data.insert::<Config>(config);
    }

    info!("Chains loaded");

    if let Err(why) = client.start() {
        error!("Client error: {:?}", why);
    }
}
