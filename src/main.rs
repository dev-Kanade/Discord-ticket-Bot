use anyhow::{anyhow, Result};
use rand::Rng;
use serenity::all::*;
use serenity::async_trait;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};


fn read_env(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

fn write_env_file(pairs: &[(&str, &str)]) -> Result<()> {
    let mut map: HashMap<String, String> = HashMap::new();
    let env_path = Path::new(".env");
    if env_path.exists() {
        let content = fs::read_to_string(env_path)?;
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    for (k, v) in pairs {
        map.insert(k.to_string(), v.to_string());
    }
    let mut out = String::new();
    for (k, v) in &map {
        out.push_str(&format!("{}={}\n", k, v));
    }
    fs::write(env_path, out)?;
    dotenvy::dotenv().ok();
    Ok(())
}

fn prompt(msg: &str) -> String {
    print!("{}", msg);
    io::stdout().flush().unwrap();
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).unwrap();
    line.trim().to_string()
}

async fn initial_setup() -> Result<String> {
    println!("=== 初回セットアップ ===");

    let token = prompt("Botトークンを入力してください: ");
    write_env_file(&[("DISCORDBOTTOKEN", &token)])?;
    println!("トークンを .env に保存しました。");

    println!("\nBotでログインしてサーバーリストを取得中...");
    let http = Http::new(&token);
    let guilds = http
        .get_guilds(None, None)
        .await
        .map_err(|e| anyhow!("サーバーリスト取得失敗: {}", e))?;

    if guilds.is_empty() {
        return Err(anyhow!("Botが参加しているサーバーが見つかりません。"));
    }

    println!("\n参加中のサーバー一覧:");
    for (i, g) in guilds.iter().enumerate() {
        println!("  [{}] {} (ID: {})", i + 1, g.name, g.id);
    }

    let server_index = loop {
        let input = prompt("サーバー番号を入力してください: ");
        match input.parse::<usize>() {
            Ok(n) if n >= 1 && n <= guilds.len() => break n - 1,
            _ => println!("1 〜 {} の数字を入力してください。", guilds.len()),
        }
    };
    let guild_id = guilds[server_index].id;
    write_env_file(&[("SERVERID", &guild_id.to_string())])?;
    println!("サーバーID {} を .env に保存しました。", guild_id);

    println!("\nお問い合わせEmbedを登録するチャンネルを指定してください。");
    let channels = http
        .get_channels(guild_id)
        .await
        .map_err(|e| anyhow!("チャンネル取得失敗: {}", e))?;

    let mut category_map: HashMap<Option<ChannelId>, Vec<&GuildChannel>> = HashMap::new();
    let mut category_names: HashMap<ChannelId, String> = HashMap::new();

    for ch in &channels {
        if ch.kind == ChannelType::Category {
            category_names.insert(ch.id, ch.name.clone());
        }
    }

    for ch in &channels {
        if ch.kind == ChannelType::Text {
            category_map
                .entry(ch.parent_id)
                .or_default()
                .push(ch);
        }
    }

    let mut text_channels: Vec<&GuildChannel> = Vec::new();
    let mut display_index = 1usize;
    let mut index_to_channel: HashMap<usize, ChannelId> = HashMap::new();

    let mut keys: Vec<Option<ChannelId>> = category_map.keys().cloned().collect();
    keys.sort_by_key(|k| k.map(|id| id.get()).unwrap_or(0));

    for key in &keys {
        match key {
            None => println!("\n[カテゴリなし]"),
            Some(cid) => {
                let name = category_names.get(cid).map(|s| s.as_str()).unwrap_or("不明");
                println!("\n[{}]", name);
            }
        }
        if let Some(chs) = category_map.get(key) {
            let mut sorted = chs.to_vec();
            sorted.sort_by_key(|c| c.position);
            for ch in sorted {
                println!("  [{}] #{}", display_index, ch.name);
                index_to_channel.insert(display_index, ch.id);
                text_channels.push(ch);
                display_index += 1;
            }
        }
    }

    let channel_id = loop {
        let input = prompt("チャンネル番号を入力してください: ");
        match input.parse::<usize>() {
            Ok(n) if index_to_channel.contains_key(&n) => break index_to_channel[&n],
            _ => println!("正しい番号を入力してください。"),
        }
    };
    write_env_file(&[("CHANNELID", &channel_id.to_string())])?;
    println!("チャンネルID {} を .env に保存しました。", channel_id);

    println!("\n「サポートチーム」ロールを作成中...");
    let role = http
        .create_role(
            guild_id,
            &EditRole::new().name("サポートチーム").mentionable(true),
            Some("Support bot setup"),
        )
        .await
        .map_err(|e| anyhow!("ロール作成失敗: {}", e))?;
    write_env_file(&[("ROLEID", &role.id.to_string())])?;
    println!("ロールID {} を .env に保存しました。", role.id);

    println!("\n「サポートチケット」カテゴリを作成中...");
    let active_cat = http
        .create_channel(
            guild_id,
            &CreateChannel::new("サポートチケット").kind(ChannelType::Category),
            Some("Support bot setup"),
        )
        .await
        .map_err(|e| anyhow!("カテゴリ作成失敗: {}", e))?;
    write_env_file(&[("ACTIVE", &active_cat.id.to_string())])?;
    println!("ACTIVEカテゴリID {} を .env に保存しました。", active_cat.id);

    println!("\n「チケットアーカイブ」カテゴリを作成中...");
    let archive_cat = http
        .create_channel(
            guild_id,
            &CreateChannel::new("チケットアーカイブ").kind(ChannelType::Category),
            Some("Support bot setup"),
        )
        .await
        .map_err(|e| anyhow!("アーカイブカテゴリ作成失敗: {}", e))?;
    write_env_file(&[("ARCHIVE", &archive_cat.id.to_string())])?;
    println!("ARCHIVEカテゴリID {} を .env に保存しました。", archive_cat.id);

    println!("\n✅ 初期設定が完了しました！");
    Ok(token)
}

#[derive(Default)]
struct BotState {
    tickets: HashMap<String, ChannelId>,
}

struct Handler {
    state: Arc<RwLock<BotState>>,
}


#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Botログイン完了: {}", ready.user.name);

        ctx.set_activity(Some(ActivityData::playing("サポート受付中")));

        let channel_id_str = read_env("CHANNELID");
        let channel_id: ChannelId = match channel_id_str.parse::<u64>() {
            Ok(id) => ChannelId::new(id),
            Err(_) => {
                error!("CHANNELIDが無効です");
                return;
            }
        };

        let embed = CreateEmbed::new()
            .title("サポートチケットを作成")
            .description("次のボタンを押してサポートチケットを作成します。")
            .color(0x5865F2);

        let button = CreateButton::new("create_ticket")
            .label("作成")
            .style(ButtonStyle::Primary);

        let components = vec![CreateActionRow::Buttons(vec![button])];

        let msg = CreateMessage::new()
            .embed(embed)
            .components(components);

        if let Err(e) = channel_id.send_message(&ctx.http, msg).await {
            error!("Embed送信失敗: {}", e);
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Component(comp) => {
                self.handle_component(&ctx, comp).await;
            }
            _ => {}
        }
    }
}

impl Handler {
    async fn handle_component(&self, ctx: &Context, mut comp: ComponentInteraction) {
        match comp.data.custom_id.as_str() {
            "create_ticket" => {
                self.handle_create_ticket(ctx, &mut comp).await;
            }
            id if id.starts_with("close_ticket_") => {
                let ticket_num = id.trim_start_matches("close_ticket_").to_string();
                self.handle_close_ticket(ctx, &mut comp, ticket_num).await;
            }
            _ => {}
        }
    }

    async fn handle_create_ticket(&self, ctx: &Context, comp: &mut ComponentInteraction) {
        let active_id_str = read_env("ACTIVE");
        let guild_id_str = read_env("SERVERID");
        let role_id_str = read_env("ROLEID");

        let active_cat_id: ChannelId = match active_id_str.parse::<u64>() {
            Ok(id) => ChannelId::new(id),
            Err(_) => {
                let _ = comp
                    .create_response(
                        ctx,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("設定エラー: ACTIVEカテゴリが見つかりません。")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }
        };

        let guild_id: GuildId = match guild_id_str.parse::<u64>() {
            Ok(id) => GuildId::new(id),
            Err(_) => {
                return;
            }
        };

        let role_id: RoleId = match role_id_str.parse::<u64>() {
            Ok(id) => RoleId::new(id),
            Err(_) => {
                return;
            }
        };
        let ticket_num: String = {
            let mut rng = rand::thread_rng();
            format!("{:06}", rng.gen_range(0..=999999))
        };

        let user_id = comp.user.id;

        let perms = vec![
            PermissionOverwrite {
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
                kind: PermissionOverwriteType::Role(guild_id.everyone_role()),
            },
            PermissionOverwrite {
                allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                deny: Permissions::empty(),
                kind: PermissionOverwriteType::Member(user_id),
            },
            PermissionOverwrite {
                allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                deny: Permissions::empty(),
                kind: PermissionOverwriteType::Role(role_id),
            },
        ];

        let new_channel = guild_id
            .create_channel(
                ctx,
                CreateChannel::new(ticket_num.clone())
                    .kind(ChannelType::Text)
                    .category(active_cat_id)
                    .permissions(perms),
            )
            .await;

        match new_channel {
            Ok(channel) => {
                {
                    let mut state = self.state.write().await;
                    state.tickets.insert(ticket_num.clone(), channel.id);
                }

                let _ = comp
                    .create_response(
                        ctx,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(format!(
                                    "チケットを作成しました！ <#{}> をご確認ください。",
                                    channel.id
                                ))
                                .ephemeral(true),
                        ),
                    )
                    .await;

                let close_button = CreateButton::new(format!("close_ticket_{}", ticket_num))
                    .label("チケットを閉じる")
                    .style(ButtonStyle::Danger);

                let embed = CreateEmbed::new()
                    .title("サポートチケット")
                    .description(format!(
                        "サポートチケット#{} を作成しました。サポートチームが返信しますのでお待ちください。また、以下のボタンを押すことでチケットを閉じることができます。",
                        ticket_num
                    ))
                    .color(0x57F287);

                let msg = CreateMessage::new()
                    .content(format!("<@{}>", user_id))
                    .embed(embed)
                    .components(vec![CreateActionRow::Buttons(vec![close_button])]);

                if let Err(e) = channel.send_message(ctx, msg).await {
                    error!("チケットチャンネルへの送信失敗: {}", e);
                }
            }
            Err(e) => {
                error!("チャンネル作成失敗: {}", e);
                let _ = comp
                    .create_response(
                        ctx,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("チャンネル作成に失敗しました。")
                                .ephemeral(true),
                        ),
                    )
                    .await;
            }
        }
    }

    async fn handle_close_ticket(
        &self,
        ctx: &Context,
        comp: &mut ComponentInteraction,
        ticket_num: String,
    ) {
        let archive_id_str = read_env("ARCHIVE");
        let archive_cat_id: ChannelId = match archive_id_str.parse::<u64>() {
            Ok(id) => ChannelId::new(id),
            Err(_) => {
                let _ = comp
                    .create_response(
                        ctx,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("設定エラー: ARCHIVEカテゴリが見つかりません。")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }
        };

        let channel_id = comp.channel_id;

        let embed = CreateEmbed::new()
            .title("クローズ")
            .description(format!(
                "チケット#{} を閉じました。まだ解決できていない場合は新しくチケットを作成し、サポートチームにチケット番号をお伝えください。",
                ticket_num
            ))
            .color(0xED4245);

        let _ = comp
            .create_response(
                ctx,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .embed(embed)
                        .ephemeral(false),
                ),
            )
            .await;

        if let Err(e) = channel_id
            .edit(ctx, EditChannel::new().category(archive_cat_id))
            .await
        {
            error!("チャンネル移動失敗: {}", e);
        }

        {
            let mut state = self.state.write().await;
            state.tickets.remove(&ticket_num);
        }
    }
}


#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    dotenvy::dotenv().ok();

    let is_first_run = !Path::new(".env").exists();

    let token = if is_first_run {
        initial_setup().await?
    } else {
        dotenvy::dotenv().ok();
        let t = read_env("DISCORDBOTTOKEN");
        if t.is_empty() {
            return Err(anyhow!(".env は存在しますが DISCORDBOTTOKEN が空です。"));
        }
        t
    };

    for key in &["SERVERID", "CHANNELID", "ROLEID", "ACTIVE", "ARCHIVE"] {
        if read_env(key).is_empty() {
            return Err(anyhow!(".env に {} が設定されていません。.env を削除して再実行してください。", key));
        }
    }

    println!("\nBotを起動中...");

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_MEMBERS;

    let state = Arc::new(RwLock::new(BotState::default()));
    let handler = Handler {
        state: Arc::clone(&state),
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| anyhow!("クライアント作成失敗: {}", e))?;

    client
        .start()
        .await
        .map_err(|e| anyhow!("Bot起動失敗: {}", e))?;

    Ok(())
}