#[cfg(windows)]
use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    config::{OcrConfig, OcrEngineKind},
    i18n,
    image::CapturedImage,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum AiOcrState {
    Ready,
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
    if command != std::ffi::OsStr::new(WORKER_ARGUMENT) {
        return None;
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
    let executable = std::env::current_exe()
        .map_err(|error| OcrFailure::Failed(format!("无法定位 OCR 程序：{error}")))?;
    let mut child = Command::new(executable)
        .arg(argument)
        .arg(&job.output)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| OcrFailure::Failed(format!("无法启动增强 OCR 检测：{error}")))?;
    let status = wait_for_worker(&mut child, "增强 OCR 准备", timeout)?;
    if !status.success() {
        return Err(OcrFailure::Failed(format_worker_failure(status)));
    }
    let response = fs::read(&job.output)
        .map_err(|error| OcrFailure::Failed(format!("无法读取增强 OCR 检测结果：{error}")))?;
    serde_json::from_slice::<AiProbeResponse>(&response)
        .map_err(|error| OcrFailure::Failed(format!("增强 OCR 检测结果格式无效：{error}")))?
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
    .map_err(|error| OcrFailure::Failed(format!("无法准备 OCR 图像：{error}")))?;

    let executable = std::env::current_exe()
        .map_err(|error| OcrFailure::Failed(format!("无法定位 OCR 程序：{error}")))?;
    let mut child = Command::new(executable)
        .arg(WORKER_ARGUMENT)
        .arg(match engine {
            OcrEngineKind::System => "system",
            OcrEngineKind::WindowsAi => "windows_ai",
        })
        .arg(&job.input)
        .arg(&job.output)
        .arg(minimum_confidence.clamp(0, 100).to_string())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| OcrFailure::Failed(format!("无法启动 OCR 子进程：{error}")))?;

    let status = wait_for_worker(&mut child, "OCR 识别", WORKER_TIMEOUT)?;

    if !status.success() {
        return Err(OcrFailure::Failed(format_worker_failure(status)));
    }
    let response = fs::read(&job.output)
        .map_err(|error| OcrFailure::Failed(format!("无法读取 OCR 结果：{error}")))?;
    serde_json::from_slice::<WorkerResponse>(&response)
        .map_err(|error| OcrFailure::Failed(format!("OCR 结果格式无效：{error}")))?
        .result
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
                return Err(OcrFailure::Failed(format!("{operation}超时")));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OcrFailure::Failed(format!(
                    "无法获取 OCR 子进程状态：{error}"
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
            _ => Err(OcrFailure::Failed("未知 OCR 引擎".to_owned())),
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
        Some(code) => format!("Windows OCR 子进程异常退出（0x{:08X}）", code as u32),
        None => "Windows OCR 子进程异常退出".to_owned(),
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
        fs::create_dir(&directory)
            .map_err(|error| OcrFailure::Failed(format!("无法创建 OCR 临时目录：{error}")))?;
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
            Self::MissingLanguagePack => i18n::text(
                "缺少可用的 Windows OCR 语言包",
                "No compatible Windows OCR language pack is installed",
            )
            .to_owned(),
            Self::Unsupported => i18n::text(
                "当前系统或程序安装方式不支持 Windows 系统 OCR",
                "Windows system OCR is not supported by this system or installation",
            )
            .to_owned(),
            Self::AiUnavailable(state) => state.message(),
            Self::Failed(message) if message.trim().is_empty() => {
                i18n::text("OCR 识别失败", "OCR failed").to_owned()
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
            Self::Ready => i18n::text(
                "可用（Windows AI OCR）",
                "Available (Windows AI OCR)",
            )
            .to_owned(),
            Self::Preparing => i18n::text(
                "正在下载并准备识别模型...",
                "Downloading and preparing the recognition model...",
            )
            .to_owned(),
            Self::ModelNotInstalled => i18n::text(
                "支持，但识别模型尚未安装",
                "Supported, but the recognition model is not installed",
            )
            .to_owned(),
            Self::Unsupported => i18n::text(
                "当前系统、硬件、驱动或策略不支持 Windows AI OCR",
                "Windows AI OCR is not supported by the current system, hardware, driver, or policy",
            )
            .to_owned(),
            Self::DisabledByUser => i18n::text(
                "Windows AI 功能已被用户禁用",
                "Windows AI features were disabled by the user",
            )
            .to_owned(),
            Self::ComponentMissing => i18n::text(
                "Windows AI OCR 组件或包身份不可用",
                "The Windows AI OCR component or package identity is unavailable",
            )
            .to_owned(),
            Self::Failed(message) => format!(
                "{}: {message}",
                i18n::text("Windows AI OCR 检测失败", "Windows AI OCR check failed")
            ),
        }
    }
}
