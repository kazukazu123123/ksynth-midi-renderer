pub mod limiter;
pub mod multi_synth;
pub mod predefined_sample;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use ksynth_core::{
    Channel,
    sample::{Sample, SampleData},
};
use limiter::Limiter;
use midi_toolkit::{
    events::MIDIEvent,
    io::MIDIFile,
    pipe,
    sequence::{
        TimeCaster,
        event::{
            cancel_tempo_events, get_channels_array_statistics, merge_events_array,
            scale_event_time,
        },
        to_vec, unwrap_items,
    },
};
use multi_synth::MultiSynth;
use predefined_sample::generate_piano_sample;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rfd::FileDialog;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

/// MIDI to WAV renderer using KSynth
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the MIDI file to render (optional, will show file dialog if not provided)
    #[arg(short = 'm', long)]
    midi_file_path: Option<String>,

    /// Sample rate for audio rendering
    #[arg(short = 's', long, default_value_t = 48000)]
    sample_rate: u32,

    /// Number of audio channels (1 for mono, 2 for stereo)
    #[arg(short = 'c', long, default_value_t = 2)]
    num_channel: u16,

    /// Maximum polyphony (number of simultaneous voices, 0 for use max voice supported on ksynth)
    #[arg(short = 'p', long, default_value_t = 512)]
    max_polyphony: usize,

    /// Number of threads to use for rendering (0 for auto-detect)
    #[arg(short = 't', long, default_value_t = 1)]
    thread_count: usize,

    /// Headless mode (use non-interactive progress-bar)
    #[arg(short = 'H', long)]
    headless: bool,

    /// Log output interval in milliseconds for headless mode (default: 1000ms)
    #[arg(long, default_value_t = 1000)]
    log_interval_ms: u64,
}

fn format_duration(duration: Duration, show_ms: bool) -> String {
    let total_seconds = duration.as_secs_f64();
    let hours = (total_seconds / 3600.0) as u64;
    let minutes = ((total_seconds % 3600.0) / 60.0) as u64;
    let seconds = (total_seconds % 60.0) as u64;

    if show_ms {
        let ms = (total_seconds.fract() * 100.0) as u64;
        format!("{:02}:{:02}:{:02}.{:02}", hours, minutes, seconds, ms)
    } else {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().rev().collect();
    let mut with_commas = String::new();

    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            with_commas.push(',');
        }
        with_commas.push(*ch);
    }

    with_commas.chars().rev().collect()
}

fn human_readable_number(n: u64) -> String {
    match n {
        n if n >= 1_000_000_000 => format!("{:.2} billion", n as f64 / 1_000_000_000.0),
        n if n >= 1_000_000 => format!("{:.2} million", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{:.2} thousand", n as f64 / 1_000.0),
        _ => n.to_string(),
    }
}

fn main() {
    // コマンドライン引数を解析
    let args = Args::parse();

    // 引数から値を取得
    let sample_rate = args.sample_rate;
    let num_channel = args.num_channel;
    let max_polyphony = if args.max_polyphony == 0 {
        ksynth_core::MAX_POLYPHONY
    } else {
        args.max_polyphony as u32
    };

    let thread_count = if args.thread_count == 0 {
        num_cpus::get()
    } else {
        args.thread_count
    };
    let headless = args.headless;

    // ヘッドレスモードでMIDIファイルパスが指定されていない場合は早期エラー
    if headless && args.midi_file_path.is_none() {
        eprintln!("Error: MIDI file path must be specified in headless mode");
        eprintln!(
            "Usage: {} --headless --midi-file-path <path>",
            std::env::args()
                .next()
                .unwrap_or_else(|| "program".to_string())
        );
        return;
    }

    if !headless {
        println!("KSynth MIDI Renderer");
        println!("====================");
    }

    // 設定を表示
    if headless {
        // Machine-readable format
        println!("sample_rate={}", sample_rate);
        println!("channels={}", num_channel);
        println!("max_polyphony={}", max_polyphony);
        println!("thread_count={}", thread_count);
        println!("log_interval_ms={}", args.log_interval_ms);
    } else {
        println!("Sample Rate: {} Hz", format_number(sample_rate as u64));
        println!("Channels: {}", num_channel);
        println!("Max Polyphony: {}", format_number(max_polyphony as u64));
        println!("Thread Count: {}", format_number(thread_count as u64));
        println!();
    }

    let ksynth_num_channel: Channel = num_channel
        .try_into()
        .expect("Failed to convert channel to KSynth Channel");

    let mut limiters = [
        Limiter::new(sample_rate as f32, 0.0, 100.0, 20.0),
        Limiter::new(sample_rate as f32, 0.0, 100.0, 20.0),
    ];

    let apply_limiter = true;
    let use_multithread = true;

    let mut peak_polyphony = 0;

    if !headless {
        println!("Creating Samples HashMap...");
    }
    let mut samples_map: HashMap<u8, Sample> = HashMap::with_capacity(128);
    if !headless {
        println!("Samples HashMap Created!");
        println!("Loading sample...");
    }

    // Precalculate sample
    let samples_vec: Vec<(u8, Sample)> = (0u8..128)
        .into_par_iter()
        .map(|key| {
            let freq = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
            let sample_count = (sample_rate as f32 * 10.0) as usize;

            let sample_vec: Vec<i16> = generate_piano_sample(sample_rate, freq, sample_count);

            let ksynth_sample_data = SampleData::Mono(sample_vec);
            let ksynth_sample = Sample::new(sample_rate as u32, ksynth_sample_data, None);

            (key, ksynth_sample)
        })
        .collect();

    for (key, sample) in samples_vec {
        samples_map.insert(key, sample);
    }

    if !headless {
        println!("Sample Loaded!");
        println!("Creating KSynth...");
    }
    let samples_arc = Arc::new(RwLock::new(samples_map));
    let mut multi_synth = MultiSynth::new(
        sample_rate,
        ksynth_num_channel,
        max_polyphony as u32,
        ((sample_rate as f64) * 0.1) as u64,
        samples_arc,
        if use_multithread { thread_count } else { 1 },
    );
    if !headless {
        println!("KSynth Ready!");
    }

    // MIDIファイルのパスを取得（引数で指定されていない場合はファイルダイアログを表示）
    let midi_path = match args.midi_file_path {
        Some(path) => {
            // パスが存在するか確認
            if !std::path::Path::new(&path).exists() {
                eprintln!("Error: MIDI file not found: {}", path);
                return;
            }
            path
        }
        None => {
            // ヘッドレスモードではファイルダイアログを表示できない
            if headless {
                eprintln!("Error: MIDI file path must be specified in headless mode");
                eprintln!(
                    "Usage: {} --headless --midi-file-path <path>",
                    std::env::args()
                        .next()
                        .unwrap_or_else(|| "program".to_string())
                );
                return;
            }

            // ファイルダイアログを表示
            let midi_file = FileDialog::new()
                .add_filter("MIDI File", &["mid", "midi"])
                .pick_file();

            match midi_file {
                Some(file) => file.as_path().to_string_lossy().to_string(),
                None => {
                    println!("No MIDI file selected. Exiting.");
                    return;
                }
            }
        }
    };

    let midi_file_name = std::path::Path::new(&midi_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();
    let midi_file_name_without_extension = std::path::Path::new(&midi_path)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();

    if headless {
        println!("midi_file={}", midi_file_name);
    } else {
        println!("Loading MIDI: {}", midi_file_name);
    }
    let midi = MIDIFile::open(midi_path, None).expect("Failed to open midi file!");
    if !headless {
        println!("MIDI Loaded!");
    }

    let ppq = midi.ppq();
    let merge_midi = || {
        pipe!(
            midi.iter_all_tracks()
            |>to_vec()
            |>merge_events_array()
            |>TimeCaster::<f64>::cast_event_delta()
            |>cancel_tempo_events(250000)
            |>scale_event_time(1.0 / ppq as f64)
            |>unwrap_items()
        )
    };

    if !headless {
        println!("Calculating MIDI Statistics");
    }
    let statistics = pipe!(
        midi.iter_all_tracks()
        |>to_vec()
        |>get_channels_array_statistics()
    )
    .expect("Failed to calculate statistics we're doomed");
    let midi_duration = statistics.calculate_total_duration(ppq);
    let note_count = statistics.note_count();
    drop(statistics);

    let total_frames = (midi_duration.as_secs_f64() * sample_rate as f64).ceil() as u64;
    if headless {
        println!("midi_duration_sec={:.2}", midi_duration.as_secs_f64());
        println!("note_count={}", note_count);
    } else {
        println!("MIDI Statistics Calculated!");
        println!("MIDI Duration: {}", format_duration(midi_duration, false));
        println!(
            "Note Count: {} ({})",
            format_number(note_count),
            human_readable_number(note_count)
        );
    }

    let pb = if !headless {
        let pb = ProgressBar::new(total_frames);
        pb.set_style(
            ProgressStyle::with_template("{msg}\n[{wide_bar:.cyan/blue}] {percent}%")
                .unwrap()
                .progress_chars("##-"),
        );
        Some(pb)
    } else {
        None
    };

    if !headless {
        println!("Preparing audio encoder...");
    }
    let spec = hound::WavSpec {
        channels: num_channel,
        sample_rate: sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer =
        hound::WavWriter::create(format!("{}.wav", midi_file_name_without_extension), spec)
            .unwrap();
    if !headless {
        println!("Audio Encoder Created!");
        println!("Rendering Started");
    } else {
        println!("rendering_started")
    }

    let rendering_start_time = Instant::now();

    let mut time_acc = 0.0;
    let mut headless_last_report_time = Instant::now();
    let headless_report_interval = Duration::from_millis(args.log_interval_ms);
    let mut total_rendered_frames: u64 = 0;

    for merged_event in merge_midi() {
        time_acc += merged_event.delta * sample_rate as f64;

        let frame_count = time_acc.floor() as usize;
        time_acc -= frame_count as f64;

        if frame_count > 0 {
            let mut synth_buffer = vec![0.0f32; frame_count * num_channel as usize];
            multi_synth.fill_buffer(synth_buffer.as_mut_slice());

            if apply_limiter {
                for (i, limiter) in limiters.iter_mut().enumerate() {
                    let channel_samples = &mut synth_buffer[i..];
                    let processed_samples = limiter.process(channel_samples);
                    channel_samples.copy_from_slice(&processed_samples);
                }
            }

            for frame in synth_buffer.chunks_exact(num_channel as usize) {
                for &sample in frame {
                    writer
                        .write_sample(sample)
                        .expect("Failed to write sample!");
                }
            }

            if let Some(ref pb) = pb {
                pb.inc(frame_count as u64);
            }
            total_rendered_frames += frame_count as u64;
        }

        if let Some(event_u32) = merged_event.event.as_u32() {
            multi_synth.queue_midi_cmd(event_u32);
        }

        let active_polyphony = multi_synth.get_polyphony();
        let max_polyphony = multi_synth.get_max_polyphony();
        let synth_rendering_time = multi_synth.get_rendering_time_ratio() * 100.0;
        let current_frames = if let Some(ref pb) = pb {
            pb.position()
        } else {
            total_rendered_frames
        };

        let current_time = Duration::from_secs_f64(current_frames as f64 / sample_rate as f64);

        if active_polyphony > peak_polyphony {
            peak_polyphony = active_polyphony;
        }

        if let Some(ref pb) = pb {
            pb.set_message(format!(
                "Time: {} / {}\nVoices: {} (Peak: {}) / {}\nRT: {:.2}%",
                format_duration(current_time, true),
                format_duration(midi_duration, true),
                format_number(active_polyphony as u64),
                format_number(peak_polyphony as u64),
                format_number(max_polyphony as u64),
                synth_rendering_time
            ));
        } else if headless && headless_last_report_time.elapsed() >= headless_report_interval {
            // Headless mode: key=value format for consistency
            println!(
                "progress: current_sec={:.2} total_sec={:.2} percent={:.1} active_voices={} max_voices={} peak_voices={} rt_percent={:.2}",
                current_time.as_secs_f64(),
                midi_duration.as_secs_f64(),
                (current_time.as_secs_f64() / midi_duration.as_secs_f64()) * 100.0,
                active_polyphony,
                max_polyphony,
                peak_polyphony,
                synth_rendering_time
            );
            headless_last_report_time = Instant::now();
        }
    }

    let duration_sec = 1;
    let frame_count = sample_rate as usize * num_channel as usize * duration_sec;
    let mut synth_buffer = vec![0.0f32; frame_count * num_channel as usize];
    multi_synth.fill_buffer(synth_buffer.as_mut_slice());

    if apply_limiter {
        for (i, limiter) in limiters.iter_mut().enumerate() {
            let channel_samples = &mut synth_buffer[i..];
            let processed_samples = limiter.process(channel_samples);
            channel_samples.copy_from_slice(&processed_samples);
        }
    }

    for frame in synth_buffer.chunks_exact(num_channel as usize) {
        for &sample in frame {
            writer
                .write_sample(sample)
                .expect("Failed to write sample!");
        }
    }

    writer.finalize().expect("Failed to finalize!");

    let rendering_end_time = Instant::now();

    let rendering_took_time = rendering_end_time.duration_since(rendering_start_time);

    if let Some(pb) = pb {
        pb.finish();
    } else if headless {
        // Final progress line
        println!(
            "progress: current_sec={:.2} total_sec={:.2} percent=100.0 active_voices=0 max_voices={} peak_voices={} rt_percent=0.00",
            midi_duration.as_secs_f64(),
            midi_duration.as_secs_f64(),
            max_polyphony,
            peak_polyphony
        );
    }
    if headless {
        println!("rendering_finished");
        println!(
            "rendering_time_sec={:.2}",
            rendering_took_time.as_secs_f64()
        );
        println!(
            "realtime_ratio={:.2}",
            midi_duration.as_secs_f64() / rendering_took_time.as_secs_f64()
        );
    } else {
        println!(
            "\nRendering finished!\nTotal time: {}\nReal-time ratio: {:.2}x",
            format_duration(rendering_took_time, true),
            midi_duration.as_secs_f64() / rendering_took_time.as_secs_f64()
        );
    }
}
