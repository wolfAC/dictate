//! Picks a local Whisper model that keeps dictation responsive on this computer.
//! Models run on the CPU, so the deciding factors are core count, chip type and memory.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Specs {
    pub cpu: String,
    pub physical_cores: usize,
    pub memory_gb: f32,
    pub apple_silicon: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub model: &'static str,
    pub reason: String,
    pub specs: Specs,
}

pub fn detect() -> Specs {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    let cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    Specs {
        physical_cores: sysinfo::System::physical_core_count().unwrap_or(threads / 2).max(1),
        memory_gb: sys.total_memory() as f32 / 1024f32.powi(3),
        apple_silicon: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        cpu: if cpu.is_empty() { "Unknown CPU".into() } else { cpu },
    }
}

/// `english` picks the English-only variant, which is faster and more accurate for English.
pub fn recommend(specs: Specs, english: bool) -> Recommendation {
    let (size, why) = if specs.memory_gb < 6.0 || specs.physical_cores < 4 {
        ("tiny", "the smallest model keeps it responsive on this hardware")
    } else if specs.apple_silicon && specs.physical_cores >= 8 {
        ("small", "Apple Silicon handles the more accurate Small model in a second or two")
    } else if !specs.apple_silicon && specs.physical_cores >= 8 && specs.memory_gb >= 16.0 {
        ("small", "enough cores for the more accurate Small model at a usable speed")
    } else {
        ("base", "Base transcribes a sentence in about a second on this CPU; larger models would lag")
    };
    let model = match (size, english) {
        ("tiny", true) => "tiny.en",
        ("small", true) => "small.en",
        ("small", false) => "small",
        (_, true) => "base.en",
        // There's no multilingual tiny in the catalog; base is the smallest.
        (_, false) => "base",
    };
    let reason = format!(
        "{}, {} cores, {:.0} GB RAM: {why}.",
        specs.cpu, specs.physical_cores, specs.memory_gb
    );
    Recommendation { model, reason, specs }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs(cores: usize, memory_gb: f32, apple_silicon: bool) -> Specs {
        Specs { cpu: "Test CPU".into(), physical_cores: cores, memory_gb, apple_silicon }
    }

    #[test]
    fn recommends_by_hardware() {
        // This Mac: 6-core Intel i7-8850H, 32 GB.
        assert_eq!(recommend(specs(6, 32.0, false), true).model, "base.en");
        assert_eq!(recommend(specs(2, 4.0, false), true).model, "tiny.en");
        assert_eq!(recommend(specs(8, 16.0, true), true).model, "small.en");
        assert_eq!(recommend(specs(12, 32.0, false), false).model, "small");
        assert_eq!(recommend(specs(2, 4.0, false), false).model, "base");
    }

    #[test]
    fn detects_something() {
        let s = detect();
        assert!(s.physical_cores >= 1 && s.memory_gb > 0.5, "{s:?}");
    }
}
