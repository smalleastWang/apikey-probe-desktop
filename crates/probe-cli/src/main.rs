use anyhow::{anyhow, Context, Result};
use apikey_probe_core::{
    infer_protocol_type, run_probe, to_json, to_markdown, to_summary, OverallConclusion,
    ProbeConfig, ProbeReport,
};
use dialoguer::{theme::ColorfulTheme, Select};
use std::{
    env, fs,
    io::{self, Read, Write},
    process::ExitCode,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(70)
        }
    }
}

async fn run() -> Result<u8> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return run_interactive().await;
    }

    if args[0] == "--help" || args[0] == "-h" {
        print_help();
        return Ok(0);
    }

    run_with_args(&args).await
}

async fn run_with_args(args: &[String]) -> Result<u8> {
    let options = if args.is_empty() || args.iter().any(|arg| arg == "--interactive" || arg == "-i")
    {
        CheckOptions::prompt()?
    } else {
        CheckOptions::parse(args)?
    };
    run_check_with_options(options).await
}

async fn run_interactive() -> Result<u8> {
    run_check_with_options(CheckOptions::prompt()?).await
}

async fn run_check_with_options(options: CheckOptions) -> Result<u8> {
    let api_key = options.read_api_key()?;
    let protocol_type = options.protocol_type()?;
    let config = ProbeConfig {
        base_url: options.base_url,
        api_key,
        model: options.model,
        protocol_type,
        provider_name: options.provider_name,
        note: options.note,
        proxy_url: options.proxy_url,
        save_api_key: false,
    };

    let progress = |progress: apikey_probe_core::ProbeProgress| {
        eprintln!(
            "[{}] {} - {}",
            progress.status_string(),
            progress.label,
            progress.message
        );
    };

    let report = run_probe(config, &progress).await?;
    let output = format_report(&report, options.format)?;

    if let Some(path) = options.out {
        fs::write(&path, output).with_context(|| format!("failed to write {path}"))?;
    } else {
        println!("{output}");
    }

    Ok(exit_code_for(report.conclusion, options.fail_on))
}

#[derive(Debug)]
struct CheckOptions {
    base_url: String,
    model: String,
    protocol: String,
    api_key: Option<String>,
    api_key_env: Option<String>,
    api_key_stdin: bool,
    provider_name: Option<String>,
    note: Option<String>,
    proxy_url: Option<String>,
    format: OutputFormat,
    out: Option<String>,
    fail_on: FailOn,
}

impl CheckOptions {
    fn prompt() -> Result<Self> {
        println!("上游 API Key / 模型验货 CLI");
        println!("按回车可使用括号内默认值，API Key 输入不会回显。");
        println!();

        let base_url = prompt_required("Base URL", None)?;
        let api_key = Some(prompt_secret_required("API Key")?);
        let model = prompt_required("模型名", None)?;
        let inferred_protocol = infer_protocol_type(&model).unwrap_or("openai-compatible");
        println!("已根据模型名推测协议类型：{inferred_protocol}");
        let protocol = prompt_select_value(
            "协议类型",
            &[
                ("OpenAI-compatible Chat Completions", "openai-compatible"),
                ("OpenAI Responses API", "openai-responses"),
                ("Anthropic Messages API", "anthropic-messages"),
                ("Google Gemini API", "google-gemini"),
            ],
            inferred_protocol,
        )?;
        let provider_name = prompt_optional("供应商名称", None)?;
        let proxy_url = prompt_optional("代理地址", Some("例如 http://127.0.0.1:7890"))?;
        let note = prompt_optional("备注", None)?;
        let format_value = prompt_select_value(
            "输出格式",
            &[
                ("摘要（直接输出到终端）", "summary"),
                ("JSON 报告", "json"),
                ("Markdown 报告", "markdown"),
            ],
            "summary",
        )?;
        let format = OutputFormat::parse(&format_value)?;
        let out = match format {
            OutputFormat::Summary => None,
            OutputFormat::Json => Some(prompt_line("输出文件路径", Some("report.json"), None)?),
            OutputFormat::Markdown => Some(prompt_line("输出文件路径", Some("report.md"), None)?),
        };
        let fail_on = FailOn::parse(&prompt_select_value(
            "退出码失败阈值",
            &[
                ("仅 FAIL 时返回失败退出码", "fail"),
                ("WARN 或 FAIL 时返回失败退出码", "warn"),
                ("永远返回成功退出码", "never"),
            ],
            "fail",
        )?)?;

        Ok(Self {
            base_url,
            model,
            protocol,
            api_key,
            api_key_env: None,
            api_key_stdin: false,
            provider_name,
            note,
            proxy_url,
            format,
            out,
            fail_on,
        })
    }

    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Self {
            base_url: String::new(),
            model: String::new(),
            protocol: "auto".to_string(),
            api_key: None,
            api_key_env: None,
            api_key_stdin: false,
            provider_name: None,
            note: None,
            proxy_url: None,
            format: OutputFormat::Summary,
            out: None,
            fail_on: FailOn::Fail,
        };

        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--base-url" => options.base_url = take_value(args, &mut index, "--base-url")?,
                "--model" => options.model = take_value(args, &mut index, "--model")?,
                "--protocol" => options.protocol = take_value(args, &mut index, "--protocol")?,
                "--api-key" => options.api_key = Some(take_value(args, &mut index, "--api-key")?),
                "--api-key-env" => {
                    options.api_key_env = Some(take_value(args, &mut index, "--api-key-env")?)
                }
                "--api-key-stdin" => options.api_key_stdin = true,
                "--interactive" | "-i" => {}
                "--provider-name" => {
                    options.provider_name =
                        optional(take_value(args, &mut index, "--provider-name")?)
                }
                "--note" => options.note = optional(take_value(args, &mut index, "--note")?),
                "--proxy-url" => {
                    options.proxy_url = optional(take_value(args, &mut index, "--proxy-url")?)
                }
                "--format" => {
                    options.format =
                        OutputFormat::parse(&take_value(args, &mut index, "--format")?)?
                }
                "--out" => options.out = Some(take_value(args, &mut index, "--out")?),
                "--fail-on" => {
                    options.fail_on = FailOn::parse(&take_value(args, &mut index, "--fail-on")?)?
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                flag => return Err(anyhow!("unknown option: {flag}")),
            }
            index += 1;
        }

        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<()> {
        if self.base_url.trim().is_empty() {
            return Err(anyhow!("--base-url is required"));
        }
        if self.model.trim().is_empty() {
            return Err(anyhow!("--model is required"));
        }

        let key_sources = [
            self.api_key.is_some(),
            self.api_key_env.is_some(),
            self.api_key_stdin,
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count();
        if key_sources != 1 {
            return Err(anyhow!(
                "provide exactly one API key source: --api-key-env, --api-key-stdin, or --api-key"
            ));
        }

        Ok(())
    }

    fn read_api_key(&self) -> Result<String> {
        let api_key = if let Some(api_key) = &self.api_key {
            api_key.clone()
        } else if let Some(name) = &self.api_key_env {
            env::var(name).with_context(|| format!("environment variable {name} is not set"))?
        } else {
            let mut api_key = String::new();
            io::stdin()
                .read_to_string(&mut api_key)
                .context("failed to read API key from stdin")?;
            api_key
        };

        let trimmed = api_key.trim().to_string();
        if trimmed.is_empty() {
            return Err(anyhow!("API key is empty"));
        }
        Ok(trimmed)
    }

    fn protocol_type(&self) -> Result<String> {
        match self.protocol.as_str() {
            "auto" => {
                let inferred = infer_protocol_type(&self.model).unwrap_or("openai-compatible");
                if inferred == "openai-compatible" && infer_protocol_type(&self.model).is_none() {
                    eprintln!(
                        "protocol auto: unable to infer from model name, using openai-compatible"
                    );
                }
                Ok(inferred.to_string())
            }
            "openai-compatible" | "openai-responses" | "anthropic-messages" | "google-gemini" => {
                Ok(self.protocol.clone())
            }
            value => Err(anyhow!("unsupported protocol: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum OutputFormat {
    Summary,
    Json,
    Markdown,
}

impl OutputFormat {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "summary" => Ok(Self::Summary),
            "json" => Ok(Self::Json),
            "markdown" | "md" => Ok(Self::Markdown),
            _ => Err(anyhow!("--format must be summary, json, or markdown")),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FailOn {
    Never,
    Warn,
    Fail,
}

impl FailOn {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "never" => Ok(Self::Never),
            "warn" => Ok(Self::Warn),
            "fail" => Ok(Self::Fail),
            _ => Err(anyhow!("--fail-on must be never, warn, or fail")),
        }
    }
}

trait ProgressStatus {
    fn status_string(&self) -> &'static str;
}

impl ProgressStatus for apikey_probe_core::ProbeProgress {
    fn status_string(&self) -> &'static str {
        match self.status {
            apikey_probe_core::StepStatus::Running => "RUNNING",
            apikey_probe_core::StepStatus::Pass => "PASS",
            apikey_probe_core::StepStatus::Warn => "WARN",
            apikey_probe_core::StepStatus::Fail => "FAIL",
        }
    }
}

fn prompt_required(label: &str, hint: Option<&str>) -> Result<String> {
    loop {
        let value = prompt_line(label, None, hint)?;
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
        println!("{label} 不能为空。");
    }
}

fn prompt_secret_required(label: &str) -> Result<String> {
    loop {
        print!("{label}: ");
        io::stdout().flush().context("failed to flush stdout")?;
        let value = rpassword::read_password().context("failed to read hidden input")?;
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
        println!("{label} 不能为空。");
    }
}

fn prompt_optional(label: &str, hint: Option<&str>) -> Result<Option<String>> {
    optional(prompt_line(label, None, hint)?).pipe(Ok)
}

fn prompt_select_value(label: &str, choices: &[(&str, &str)], default: &str) -> Result<String> {
    let labels = choices.iter().map(|(label, _)| *label).collect::<Vec<_>>();
    let default_index = choices
        .iter()
        .position(|(_, value)| *value == default)
        .unwrap_or(0);

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(label)
        .items(&labels)
        .default(default_index)
        .interact()
        .with_context(|| format!("failed to read selection for {label}"))?;

    Ok(choices[selection].1.to_string())
}

fn prompt_line(label: &str, default: Option<&str>, hint: Option<&str>) -> Result<String> {
    print!("{label}");
    if let Some(default) = default {
        print!(" [{default}]");
    }
    if let Some(hint) = hint {
        print!(" ({hint})");
    }
    print!(": ");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("failed to read terminal input")?;
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    if value.trim().is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(value)
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

fn format_report(report: &ProbeReport, format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Summary => Ok(to_summary(report)),
        OutputFormat::Json => to_json(report).context("failed to serialize JSON"),
        OutputFormat::Markdown => Ok(to_markdown(report)),
    }
}

fn exit_code_for(conclusion: OverallConclusion, fail_on: FailOn) -> u8 {
    match (conclusion, fail_on) {
        (_, FailOn::Never) => 0,
        (OverallConclusion::Fail, _) => 2,
        (OverallConclusion::Warn, FailOn::Warn) => 1,
        _ => 0,
    }
}

fn take_value(args: &[String], index: &mut usize, name: &str) -> Result<String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| anyhow!("{name} requires a value"))
}

fn optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn print_help() {
    println!(
        r#"apikey-probe

Usage:
  apikey-probe
  apikey-probe [options]

Required:
  --base-url <url>                  Upstream base URL
  --model <name>                    Model name
  --api-key-env <name>              Read API key from environment variable
  --api-key-stdin                   Read API key from stdin
  --api-key <key>                   Read API key from argument, not recommended

Options:
  --interactive, -i                 Prompt for fields in the terminal
  --protocol <value>                auto, openai-compatible, openai-responses,
                                    anthropic-messages, google-gemini
                                    default: auto
  --provider-name <name>            Optional provider name for report archive
  --proxy-url <url>                 HTTP proxy URL
  --note <text>                     Optional note
  --format <value>                  summary, json, markdown
                                    default: summary
  --out <path>                      Write output to file instead of stdout
  --fail-on <value>                 never, warn, fail
                                    default: fail

Examples:
  apikey-probe
  apikey-probe --interactive

  apikey-probe --base-url https://api.example.com/v1 \
    --api-key-env UPSTREAM_API_KEY --model gpt-4o --format markdown --out report.md

  printf "%s" "$UPSTREAM_API_KEY" | apikey-probe \
    --base-url https://api.example.com/v1 --api-key-stdin --model claude-3-5-sonnet-latest"#
    );
}
