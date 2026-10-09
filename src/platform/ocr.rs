#[cfg(windows)]
use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use crate::config::LanguageMode;
use crate::{
    config::{OcrConfig, OcrEngineKind},
    i18n,
    image::CapturedImage,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum AiOcrState {
    Ready,
    Checking,
    Preparing,
    ModelNotInstalled,
    Unsupported,
    DisabledByUser,
    ComponentMissing,
    Failed(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum OcrFailure {
    MissingLanguagePack,
    Unsupported,
    AiUnavailable(AiOcrState),
    Failed(String),
}

pub trait OcrEngine {
    fn availability(&self) -> Result<(), OcrFailure>;
    fn recognize(&self, image: &CapturedImage) -> Result<String, OcrFailure>;
}

#[cfg(windows)]
pub fn system_engine() -> impl OcrEngine {
    super::windows::ocr::WindowsOcrEngine
}

#[cfg(windows)]
pub fn system_availability() -> Result<(), OcrFailure> {
    system_engine().availability()
}

#[cfg(windows)]
pub fn ai_availability() -> Result<AiOcrState, OcrFailure> {
    run_ai_state_isolated(AI_PROBE_ARGUMENT, Duration::from_secs(30))
}

#[cfg(windows)]
pub fn prepare_ai() -> Result<AiOcrState, OcrFailure> {
    run_ai_state_isolated(AI_PREPARE_ARGUMENT, Duration::from_secs(30 * 60))
}

#[cfg(windows)]
pub fn recognize(image: &CapturedImage, config: &OcrConfig) -> Result<String, OcrFailure> {
    recognize_isolated(image, config.engine, config.minimum_confidence)
}

#[cfg(windows)]
const WORKER_ARGUMENT: &str = "--gridstart-system-ocr-worker";
#[cfg(windows)]
const AI_PROBE_ARGUMENT: &str = "--gridstart-ai-ocr-probe";
#[cfg(windows)]
const AI_PREPARE_ARGUMENT: &str = "--gridstart-ai-ocr-prepare";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const WORKER_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(windows)]
const WORKER_LANGUAGE_ENV: &str = "SHITU_UI_LANGUAGE";

#[cfg(windows)]
#[derive(Deserialize, Serialize)]
struct WorkerResponse {
    result: Result<String, OcrFailure>,
}

#[cfg(windows)]
#[derive(Deserialize, Serialize)]
struct AiProbeResponse {
    result: Result<AiOcrState, OcrFailure>,
}

#[cfg(windows)]
pub fn worker_exit_code() -> Option<i32> {
    let mut arguments = std::env::args_os();
    let _executable = arguments.next();
    let command = arguments.next()?;

    if ![WORKER_ARGUMENT, AI_PROBE_ARGUMENT, AI_PREPARE_ARGUMENT]
        .iter()
        .any(|argument| command == std::ffi::OsStr::new(argument))
    {
        return None;
    }
    let Some(language) = std::env::var(WORKER_LANGUAGE_ENV)
        .ok()
        .as_deref()
        .and_then(worker_language)
    else {
        return Some(2);
    };
    // Worker processes do not create UI components. Prepare only the Rust
    // catalog, using the resolved language passed by the parent process.
    i18n::prepare(language);

    if command == std::ffi::OsStr::new(AI_PROBE_ARGUMENT)
        || command == std::ffi::OsStr::new(AI_PREPARE_ARGUMENT)
    {
        let output = arguments.next();
        if arguments.next().is_some() {
            return Some(2);
        }
        return Some(match output {
            Some(output) => run_ai_state_worker(
                Path::new(&output),
                command == std::ffi::OsStr::new(AI_PREPARE_ARGUMENT),
            ),
            None => 2,
        });
    }
    let engine = arguments.next();
    let input = arguments.next();
    let output = arguments.next();
    let minimum_confidence = arguments.next();
    if arguments.next().is_some() {
        return Some(2);
    }
    Some(match (engine, input, output, minimum_confidence) {
        (Some(engine), Some(input), Some(output), Some(minimum_confidence)) => run_worker(
            &engine,
            Path::new(&input),
            Path::new(&output),
            minimum_confidence.to_string_lossy().parse().unwrap_or(0),
        ),
        _ => 2,
    })
}

#[cfg(windows)]
fn run_ai_state_isolated(argument: &str, timeout: Duration) -> Result<AiOcrState, OcrFailure> {
    let job = OcrJob::create()?;
    let executable = std::env::current_exe().map_err(|error| {
        OcrFailure::Failed(format!(
            "{}: {error}",
            crate::i18n::text("无法定位 OCR 程序")
        ))
    })?;
    let mut child = worker_command(&executable, i18n::current_language())
        .arg(argument)
        .arg(&job.output)
        .spawn()
        .map_err(|error| {
            OcrFailure::Failed(format!(
                "{}: {error}",
                crate::i18n::text("无法启动增强 OCR 检测")
            ))
        })?;
    let status = wait_for_worker(&mut child, crate::i18n::text("增强 OCR 准备"), timeout)?;
    if !status.success() {
        return Err(OcrFailure::Failed(format_worker_failure(status)));
    }
    let response = fs::read(&job.output).map_err(|error| {
        OcrFailure::Failed(format!(
            "{}: {error}",
            crate::i18n::text("无法读取增强 OCR 检测结果")
        ))
    })?;
    serde_json::from_slice::<AiProbeResponse>(&response)
        .map_err(|error| {
            OcrFailure::Failed(format!(
                "{}: {error}",
                crate::i18n::text("增强 OCR 检测结果格式无效")
            ))
        })?
        .result
}

#[cfg(windows)]
fn recognize_isolated(
    image: &CapturedImage,
    engine: OcrEngineKind,
    minimum_confidence: u8,
) -> Result<String, OcrFailure> {
    let job = OcrJob::create()?;
    image::save_buffer_with_format(
        &job.input,
        &image.rgba_bytes(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| {
        OcrFailure::Failed(format!(
            "{}: {error}",
            crate::i18n::text("无法准备 OCR 图像")
        ))
    })?;

    let executable = std::env::current_exe().map_err(|error| {
        OcrFailure::Failed(format!(
            "{}: {error}",
            crate::i18n::text("无法定位 OCR 程序")
        ))
    })?;
    let mut child = worker_command(&executable, i18n::current_language())
        .arg(WORKER_ARGUMENT)
        .arg(match engine {
            OcrEngineKind::System => "system",
            OcrEngineKind::WindowsAi => "windows_ai",
        })
        .arg(&job.input)
        .arg(&job.output)
        .arg(minimum_confidence.clamp(0, 100).to_string())
        .spawn()
        .map_err(|error| {
            OcrFailure::Failed(format!(
                "{}: {error}",
                crate::i18n::text("无法启动 OCR 子进程")
            ))
        })?;

    let status = wait_for_worker(&mut child, i18n::text("OCR 识别"), WORKER_TIMEOUT)?;

    if !status.success() {
        return Err(OcrFailure::Failed(format_worker_failure(status)));
    }
    let response = fs::read(&job.output).map_err(|error| {
        OcrFailure::Failed(format!(
            "{}: {error}",
            crate::i18n::text("无法读取 OCR 结果")
        ))
    })?;
    serde_json::from_slice::<WorkerResponse>(&response)
        .map_err(|error| {
            OcrFailure::Failed(format!(
                "{}: {error}",
                crate::i18n::text("OCR 结果格式无效")
            ))
        })?
        .result
}

#[cfg(windows)]
fn worker_command(executable: &Path, language: LanguageMode) -> Command {
    assert_ne!(
        language,
        LanguageMode::System,
        "worker language must be resolved"
    );
    let mut command = Command::new(executable);
    command.env(
        WORKER_LANGUAGE_ENV,
        serde_json::to_string(&language).expect("serialize OCR worker language"),
    );
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(windows)]
fn worker_language(value: &str) -> Option<LanguageMode> {
    serde_json::from_str(value)
        .ok()
        .filter(|language| *language != LanguageMode::System)
}

#[cfg(windows)]
fn wait_for_worker(
    child: &mut std::process::Child,
    operation: &str,
    timeout: Duration,
) -> Result<ExitStatus, OcrFailure> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OcrFailure::Failed(format!(
                    "{operation}: {}",
                    i18n::text("操作超时")
                )));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OcrFailure::Failed(format!(
                    "{}: {error}",
                    crate::i18n::text("无法获取 OCR 子进程状态")
                )));
            }
        }
    }
}

#[cfg(windows)]
fn run_worker(
    engine: &std::ffi::OsStr,
    input: &Path,
    output: &Path,
    minimum_confidence: u8,
) -> i32 {
    let result = CapturedImage::from_file(input, 0, 0)
        .map_err(|error| OcrFailure::Failed(error.to_string()))
        .and_then(|image| match engine.to_string_lossy().as_ref() {
            "system" => super::windows::ocr::WindowsOcrEngine.recognize(&image),
            "windows_ai" => super::windows::windows_ai_ocr::recognize(&image, minimum_confidence),
            _ => Err(OcrFailure::Failed(
                crate::i18n::text("未知 OCR 引擎").to_owned(),
            )),
        });
    let response = WorkerResponse { result };
    match serde_json::to_vec(&response)
        .map_err(|error| error.to_string())
        .and_then(|bytes| fs::write(output, bytes).map_err(|error| error.to_string()))
    {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

#[cfg(windows)]
fn run_ai_state_worker(output: &Path, prepare: bool) -> i32 {
    let response = AiProbeResponse {
        result: if prepare {
            super::windows::windows_ai_ocr::prepare()
        } else {
            super::windows::windows_ai_ocr::availability()
        },
    };
    match serde_json::to_vec(&response)
        .map_err(|error| error.to_string())
        .and_then(|bytes| fs::write(output, bytes).map_err(|error| error.to_string()))
    {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

#[cfg(windows)]
fn format_worker_failure(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!(
            "{} (0x{:08X})",
            i18n::text("Windows OCR 子进程异常退出"),
            code as u32
        ),
        None => crate::i18n::text("Windows OCR 子进程异常退出").to_owned(),
    }
}

#[cfg(windows)]
struct OcrJob {
    directory: PathBuf,
    input: PathBuf,
    output: PathBuf,
}

#[cfg(windows)]
impl OcrJob {
    fn create() -> Result<Self, OcrFailure> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("gridstart-ocr-{}-{nonce}", std::process::id()));
        fs::create_dir(&directory).map_err(|error| {
            OcrFailure::Failed(format!(
                "{}: {error}",
                crate::i18n::text("无法创建 OCR 临时目录")
            ))
        })?;
        Ok(Self {
            input: directory.join("input.png"),
            output: directory.join("result.json"),
            directory,
        })
    }
}

#[cfg(windows)]
impl Drop for OcrJob {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

impl OcrFailure {
    pub fn message(&self) -> String {
        match self {
            Self::MissingLanguagePack => i18n::text("缺少可用的 Windows OCR 语言包").to_owned(),
            Self::Unsupported => {
                i18n::text("当前系统或程序安装方式不支持 Windows 系统 OCR").to_owned()
            }
            Self::AiUnavailable(state) => state.message(),
            Self::Failed(message) if message.trim().is_empty() => {
                i18n::text("OCR 识别失败").to_owned()
            }
            Self::Failed(message) => message.clone(),
        }
    }
}

impl AiOcrState {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn can_install(&self) -> bool {
        matches!(self, Self::ModelNotInstalled)
    }

    pub fn message(&self) -> String {
        match self {
            Self::Ready => i18n::text("可用（Windows AI OCR）").to_owned(),
            Self::Checking => i18n::text("正在检测 Windows AI OCR...").to_owned(),
            Self::Preparing => i18n::text("正在下载并准备识别模型...").to_owned(),
            Self::ModelNotInstalled => i18n::text("支持，但识别模型尚未安装").to_owned(),
            Self::Unsupported => {
                i18n::text("当前系统、硬件、驱动或策略不支持 Windows AI OCR").to_owned()
            }
            Self::DisabledByUser => i18n::text("Windows AI 功能已被用户禁用").to_owned(),
            Self::ComponentMissing => i18n::text("Windows AI OCR 组件或包身份不可用").to_owned(),
            Self::Failed(message) => {
                format!("{}: {message}", i18n::text("Windows AI OCR 检测失败"))
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{LanguageMode, WORKER_LANGUAGE_ENV, worker_command, worker_language};

    #[test]
    fn ocr_worker_inherits_every_resolved_language() {
        for mode in LanguageMode::ALL[1..].iter().copied() {
            let command = worker_command(std::path::Path::new("ShiTu.exe"), mode);
            let value = command
                .get_envs()
                .find(|(key, _)| *key == WORKER_LANGUAGE_ENV)
                .and_then(|(_, value)| value)
                .unwrap()
                .to_str()
                .unwrap();
            assert_eq!(worker_language(value), Some(mode));
        }
    }

    #[test]
    fn invalid_or_unresolved_worker_languages_are_rejected() {
        for value in ["", "\"system\"", "\"unknown\"", "ru"] {
            assert_eq!(worker_language(value), None);
        }
    }
}
