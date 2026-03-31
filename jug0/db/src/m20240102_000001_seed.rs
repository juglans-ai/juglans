use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const SYSTEM_USER_ID: &str = "00000000-0000-0000-0000-000000000001";

/// RSA public key for juglans_official org (used to verify org-signed RS256 JWTs)
const ORG_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAvUS5cDiW1lL964nIa8qZ
glYUTDKsIhU+TtdOMZAw/2RBSIKyTBML4H1bEcL/ulIZTzW7zlcSjj/ab6WmBS8u
YcZijxLERJg6Hy6zNLtjyjnwglheSji3QHIoq6UR7Fl5ZmoUvpaSygxxQvA/DFtt
nNI0xC7WwOxDxnPU6EfPd2mfxhrI+xowPvhtv1wIoeVmdbW5yske/54tyHLNMAcv
4xNv+cFxPTE1LavhPpU93z8HO5uBiph8aTuyn1gKclk3Izk48YwSn3JIvX0tlM6T
xVZcfpZPd6syzJnlaegEbpHVxB/OWcAe8XEg/igB+HSQ09My9xriERmgvSnTutoA
+QIDAQAB
-----END PUBLIC KEY-----";

/// SHA256 hash of the org API key (auth.rs hash_key() uses SHA256, NOT bcrypt)
const ORG_API_KEY_HASH: &str = "3dd362edce122cde10b1d6722146f0b8aa4f47b50bfa9cd198aefb8ebba8ac42";

/// SHA256 hash of the system CLI API key: jug0_sk_LpNCBvWgPxHFNjm5JgCec8ZdbiqTckVL
const SYSTEM_CLI_KEY_HASH: &str =
    "e740e0c635a4795eb0f1555ed5b96f406f2c93233ab6d717de5664ffaca5245d";

// All seed INSERT statements. Each uses INSERT OR IGNORE (SQLite) / ON CONFLICT
// DO NOTHING (PG) pattern via separate SQL per backend for idempotency.

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ==========================================
        // 1. Organization (with RSA public key for JWT verification)
        // ==========================================
        db.execute_unprepared(&format!(
            "INSERT INTO organizations (id, name, api_key_hash, public_key, key_algorithm) VALUES (\
             'juglans_official', 'Juglans Official', \
             '{hash}', '{pk}', 'RS256') \
             ON CONFLICT (id) DO NOTHING",
            hash = ORG_API_KEY_HASH,
            pk = ORG_PUBLIC_KEY,
        ))
        .await?;

        // ==========================================
        // 2. Users
        // ==========================================

        // Admin user
        db.execute_unprepared(
            "INSERT INTO users (id, org_id, name, username, email, password_hash, role) VALUES (\
             '00000000-0000-0000-0000-000000000000', 'juglans_official', \
             'System Admin', 'admin', 'admin@juglans.io', \
             '$2b$12$LJ3m4ys3Lk0TSwHiPbBAhOkz6KLamMStJfGGxMFPJOFMfNe7YqHWm', 'admin') \
             ON CONFLICT (id) DO NOTHING",
        )
        .await?;

        // System user (for public prompts/agents)
        db.execute_unprepared(&format!(
            "INSERT INTO users (id, org_id, name, username, role) VALUES (\
             '{}', 'juglans_official', 'Juglans Official', 'juglans', 'system') \
             ON CONFLICT (id) DO NOTHING",
            SYSTEM_USER_ID
        ))
        .await?;

        // ==========================================
        // 3. System Prompts
        // ==========================================

        let prompts = [
            ("00000000-0000-0000-0000-000000000010", "system-default",
             "Default System Prompt",
             "You are Juglans, an intelligent AI trading assistant. Help users analyze markets, execute trades, and understand financial data.",
             "system", r#"["system", "default"]"#),

            ("00000000-0000-0000-0000-000000000011", "market-analyst",
             "Market Analyst",
             "You are a professional market analyst. Provide detailed technical and fundamental analysis of stocks, crypto, and other assets. Use charts, indicators, and market data to support your analysis.",
             "agent", r#"["agent", "analysis", "markets"]"#),

            ("00000000-0000-0000-0000-000000000012", "trade-executor",
             "Trade Executor",
             "You are a trade execution specialist. Help users place orders, manage positions, and optimize execution. Always confirm order details before execution and explain potential risks.",
             "agent", r#"["agent", "trading", "execution"]"#),

            ("00000000-0000-0000-0000-000000000013", "risk-manager",
             "Risk Manager",
             "You are a risk management expert. Analyze portfolio exposure, calculate position sizes, set stop-losses, and help users manage their trading risk. Always prioritize capital preservation.",
             "agent", r#"["agent", "risk", "portfolio"]"#),

            ("00000000-0000-0000-0000-000000000014", "news-summarizer",
             "News Summarizer",
             "You are a financial news analyst. Summarize market-moving news, earnings reports, and economic events. Highlight potential trading opportunities and risks from news flow.",
             "skill", r#"["skill", "news", "analysis"]"#),

            ("00000000-0000-0000-0000-000000000015", "chart-reader",
             "Chart Pattern Reader",
             "You are a technical analysis expert specializing in chart patterns. Identify support/resistance levels, trend lines, and classic patterns like head-and-shoulders, flags, and wedges.",
             "skill", r#"["skill", "technical", "charts"]"#),

            ("00000000-0000-0000-0000-000000000016", "options-analyst",
             "Options Strategy Analyst",
             "You are an options trading specialist. Analyze options chains, Greeks, implied volatility, and help construct multi-leg strategies. Explain complex options concepts in simple terms.",
             "skill", r#"["skill", "options", "derivatives"]"#),
        ];

        for (id, slug, name, content, ptype, tags) in prompts {
            let sql = format!(
                "INSERT INTO prompts (id, slug, org_id, user_id, name, content, type, is_system, is_public, tags) VALUES (\
                 '{id}', '{slug}', 'juglans_official', '{uid}', \
                 '{name}', '{content}', '{ptype}', true, true, '{tags}') \
                 ON CONFLICT (id) DO NOTHING",
                uid = SYSTEM_USER_ID,
            );
            db.execute_unprepared(&sql).await?;
        }

        // ==========================================
        // 4. System Agents
        // ==========================================

        let agents: &[(&str, &str, &str, &str, &str, &str, f32)] = &[
            ("00000000-0000-0000-0000-000000000020", "default",
             "Juglans AI",
             "Your intelligent trading companion. General-purpose assistant for market analysis and trading.",
             "deepseek", "00000000-0000-0000-0000-000000000010", 0.7),

            ("00000000-0000-0000-0000-000000000021", "analyst",
             "Market Analyst",
             "Professional market analysis with technical and fundamental insights. Perfect for research and due diligence.",
             "gpt-4o", "00000000-0000-0000-0000-000000000011", 0.5),

            ("00000000-0000-0000-0000-000000000022", "trader",
             "Trade Assistant",
             "Your execution partner. Helps place orders, manage positions, and optimize trade timing.",
             "claude-3-5-sonnet", "00000000-0000-0000-0000-000000000012", 0.3),

            ("00000000-0000-0000-0000-000000000023", "risk-bot",
             "Risk Guardian",
             "Keeps your portfolio safe. Monitors exposure, calculates position sizes, and sets protective stops.",
             "gpt-4o-mini", "00000000-0000-0000-0000-000000000013", 0.4),

            ("00000000-0000-0000-0000-000000000024", "options-pro",
             "Options Pro",
             "Options trading specialist. Analyzes chains, Greeks, and helps build multi-leg strategies.",
             "claude-3-5-sonnet", "00000000-0000-0000-0000-000000000016", 0.5),
        ];

        for (id, slug, name, desc, model, prompt_id, temp) in agents {
            let sql = format!(
                "INSERT INTO agents (id, slug, org_id, user_id, name, description, default_model, system_prompt_id, temperature, is_public) VALUES (\
                 '{id}', '{slug}', 'juglans_official', '{uid}', \
                 '{name}', '{desc}', '{model}', '{prompt_id}', {temp}, true) \
                 ON CONFLICT (id) DO NOTHING",
                uid = SYSTEM_USER_ID,
            );
            db.execute_unprepared(&sql).await?;
        }

        // ==========================================
        // 5. System Workflows
        // ==========================================

        let workflows: &[(&str, &str, &str, &str, &str, bool)] = &[
            ("00000000-0000-0000-0000-000000000030", "morning-brief",
             "Morning Market Brief",
             "Automated daily market summary delivered before market open. Includes overnight moves, key levels, and news highlights.",
             "/webhooks/morning-brief", true),

            ("00000000-0000-0000-0000-000000000031", "earnings-alert",
             "Earnings Alert Pipeline",
             "Monitors earnings calendar and triggers analysis when companies report. Summarizes results and market reaction.",
             "/webhooks/earnings", true),

            ("00000000-0000-0000-0000-000000000032", "price-alert",
             "Price Alert System",
             "Configurable price alerts with multi-channel notifications. Triggers when assets cross specified thresholds.",
             "/webhooks/price-alert", true),

            ("00000000-0000-0000-0000-000000000033", "portfolio-rebalance",
             "Portfolio Rebalancer",
             "Automated portfolio rebalancing workflow. Analyzes drift from target allocation and generates rebalance orders.",
             "/webhooks/rebalance", false),

            ("00000000-0000-0000-0000-000000000034", "sentiment-scanner",
             "Sentiment Scanner",
             "Scans social media and news for sentiment shifts. Aggregates signals and alerts on significant changes.",
             "/webhooks/sentiment", true),
        ];

        for (id, slug, name, desc, url, active) in workflows {
            let sql = format!(
                "INSERT INTO workflows (id, slug, org_id, user_id, name, description, endpoint_url, is_active, is_public) VALUES (\
                 '{id}', '{slug}', 'juglans_official', '{uid}', \
                 '{name}', '{desc}', '{url}', {active}, true) \
                 ON CONFLICT (id) DO NOTHING",
                uid = SYSTEM_USER_ID,
            );
            db.execute_unprepared(&sql).await?;
        }

        // ==========================================
        // 6. System CLI API Key
        // ==========================================
        db.execute_unprepared(&format!(
            "INSERT INTO api_keys (id, user_id, name, prefix, key_hash) VALUES (\
             '00000000-0000-0000-0000-000000000040', '{uid}', \
             'System CLI Key', 'jug0_sk_', '{hash}') \
             ON CONFLICT (id) DO NOTHING",
            uid = SYSTEM_USER_ID,
            hash = SYSTEM_CLI_KEY_HASH,
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Delete in reverse dependency order
        db.execute_unprepared(
            "DELETE FROM api_keys WHERE id = '00000000-0000-0000-0000-000000000040'",
        )
        .await?;

        db.execute_unprepared(
            "DELETE FROM workflows WHERE org_id = 'juglans_official' AND id IN (\
             '00000000-0000-0000-0000-000000000030',\
             '00000000-0000-0000-0000-000000000031',\
             '00000000-0000-0000-0000-000000000032',\
             '00000000-0000-0000-0000-000000000033',\
             '00000000-0000-0000-0000-000000000034')",
        )
        .await?;

        db.execute_unprepared(
            "DELETE FROM agents WHERE org_id = 'juglans_official' AND id IN (\
             '00000000-0000-0000-0000-000000000020',\
             '00000000-0000-0000-0000-000000000021',\
             '00000000-0000-0000-0000-000000000022',\
             '00000000-0000-0000-0000-000000000023',\
             '00000000-0000-0000-0000-000000000024')",
        )
        .await?;

        db.execute_unprepared(
            "DELETE FROM prompts WHERE org_id = 'juglans_official' AND id IN (\
             '00000000-0000-0000-0000-000000000010',\
             '00000000-0000-0000-0000-000000000011',\
             '00000000-0000-0000-0000-000000000012',\
             '00000000-0000-0000-0000-000000000013',\
             '00000000-0000-0000-0000-000000000014',\
             '00000000-0000-0000-0000-000000000015',\
             '00000000-0000-0000-0000-000000000016')",
        )
        .await?;

        db.execute_unprepared(
            "DELETE FROM users WHERE org_id = 'juglans_official' AND id IN (\
             '00000000-0000-0000-0000-000000000000',\
             '00000000-0000-0000-0000-000000000001')",
        )
        .await?;

        db.execute_unprepared("DELETE FROM organizations WHERE id = 'juglans_official'")
            .await?;

        Ok(())
    }
}
