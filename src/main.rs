pub mod limiter;
pub mod multi_synth;
pub mod predefined_drum_samples;
pub mod predefined_sample;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use ksynth_core::{
    Channel,
    drum_kit::DrumKit,
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
use predefined_drum_samples::{
    generate_acoustic_bass_drum_sample, generate_crash_cymbal_sample,
    generate_electric_snare_sample, generate_hand_clap_sample, generate_hihat_sample,
    generate_kick_sample, generate_pedal_hihat_sample, generate_ride_cymbal_sample,
    generate_side_stick_sample, generate_snare_sample,
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rfd::FileDialog;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

/// MIDI to WAV renderer using KSynth
#[derive(Parser, Debug)]
struct Args {
    /// Path to the MIDI file to render (optional, will show file dialog if not provided)
    #[arg(short = 'm', long)]
    midi_file_path: Option<String>,

    /// Path to the sample folder (optional, if not provided, will use the default precalculated samples)
    #[arg(short = 's', long)]
    sample_folder_path: Option<String>,

    /// Format string for sample files (e.g. "SAMPLE_{key}.wav" or "{key}.wav")
    #[arg(short = 'f', long, default_value = "{key}.wav")]
    sample_format: String,

    /// Sample rate for audio rendering
    #[arg(short = 'r', long, default_value_t = 48000)]
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

    /// Log output interval in milliseconds for headless mode
    #[arg(long, default_value_t = 1000)]
    log_interval_ms: u64,

    /// Earrape noise simulation mode (like casting f32 -> s16 on C language)
    #[arg(long)]
    earrape_noise_mode: bool,

    /// Disable limiter
    #[arg(long)]
    disable_limiter: bool,

    /// Maximum rendering speed (0.0 for no limit, values between 0.0 and 1.0 will be treated as 1.0, 1.0 for realtime, higher values for faster rendering)
    #[arg(long, default_value_t = 0.0)]
    max_render_speed: f64,
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

    let sample_folder_path = args.sample_folder_path;

    // 引数から値を取得
    let sample_rate = args.sample_rate;
    let num_channel = args.num_channel;
    let max_polyphony = if args.max_polyphony == 0 {
        ksynth_core::MAX_POLYPHONY
    } else {
        (args.max_polyphony as u32).min(ksynth_core::MAX_POLYPHONY)
    };

    let thread_count = if args.thread_count == 0 {
        num_cpus::get()
    } else {
        args.thread_count
    };
    let headless = args.headless;
    let earrape_noise_mode = args.earrape_noise_mode;
    let max_render_speed = if args.max_render_speed > 0.0 && args.max_render_speed < 1.0 {
        1.0
    } else {
        args.max_render_speed
    };

    // ヘッドレスモードでMIDIファイルパスが指定されていない場合は早期エラー
    if headless && args.midi_file_path.is_none() {
        eprintln!("error MIDI file path must be specified in headless mode");
        std::process::exit(1);
    }

    if !headless {
        println!("KSynth MIDI Renderer");
        println!("====================");
    }

    // 設定を表示
    if headless {
        // Machine-readable format
        eprintln!("sample_rate={}", sample_rate);
        eprintln!("channels={}", num_channel);
        eprintln!("limiter_disabled: {}", args.disable_limiter);
        eprintln!("max_polyphony={}", max_polyphony);
        eprintln!("thread_count={}", thread_count);
        eprintln!("log_interval_ms={}", args.log_interval_ms);
        eprintln!(
            "sample_folder_path={}",
            sample_folder_path.as_deref().unwrap_or("<NOT SET>")
        );
        eprintln!("earrape_noise_mode={}", earrape_noise_mode);
        eprintln!("max_render_speed={}", max_render_speed);
    } else {
        println!("Sample Rate: {} Hz", format_number(sample_rate as u64));
        println!("Channels: {}", num_channel);
        println!("Limiter Disabled: {}", args.disable_limiter);
        println!("Max Polyphony: {}", format_number(max_polyphony as u64));
        println!("Thread Count: {}", format_number(thread_count as u64));
        println!(
            "Sample Folder Path: {}",
            sample_folder_path.as_deref().unwrap_or("<NOT SET>")
        );
        println!("Earrape noise mode: {}", earrape_noise_mode);
        println!("Max Render Speed: {}", max_render_speed);
        println!();
    }

    let ksynth_num_channel: Channel = num_channel
        .try_into()
        .expect("Failed to convert channel to KSynth Channel");

    let apply_limiter = !args.disable_limiter;

    let mut limiters = if apply_limiter {
        Some([
            Limiter::new(sample_rate as f32, 0.0, 100.0, 20.0),
            Limiter::new(sample_rate as f32, 0.0, 100.0, 20.0),
        ])
    } else {
        None
    };

    let use_multithread = true;

    let mut peak_polyphony = 0;

    if !headless {
        println!("Creating Samples HashMap...");
    } else {
        eprintln!("creating_samples_hashmap");
    }
    let mut samples_map: HashMap<u8, Sample> = HashMap::with_capacity(128);
    let mut drum_kit: Option<DrumKit> = None;
    if !headless {
        println!("Samples HashMap Created!");
        println!("Loading sample...");
    } else {
        eprintln!("created_samples_hashmap");
        eprintln!("loading_sample");
    }

    if let Some(path) = &sample_folder_path {
        if !headless {
            println!("Loading samples from folder: {}", path);
        } else {
            eprintln!("loading_samples_from_folder={}", path);
        }
        let samples_vec: Vec<(u8, Sample)> = (0u8..128)
            .into_par_iter()
            .filter_map(|key| {
                let sample_path = format!(
                    "{}/{}",
                    path,
                    args.sample_format.replace("{key}", &key.to_string())
                );
                let file = std::fs::File::open(&sample_path).ok()?;
                let mut reader = hound::WavReader::new(file).ok()?;
                let spec = reader.spec();
                let sample_rate = spec.sample_rate;
                let channels = spec.channels;

                let sample_data = match (channels, spec.sample_format) {
                    (1, hound::SampleFormat::Float) => {
                        let samples = reader
                            .samples::<f32>()
                            .map(|s| s.unwrap() as i16)
                            .collect::<Vec<_>>();
                        SampleData::Mono(samples)
                    }
                    (1, hound::SampleFormat::Int) => {
                        let samples = reader
                            .samples::<i16>()
                            .map(|s| s.unwrap())
                            .collect::<Vec<_>>();
                        SampleData::Mono(samples)
                    }
                    (2, hound::SampleFormat::Float) => {
                        let samples = reader
                            .samples::<f32>()
                            .map(|s| s.unwrap() as i16)
                            .collect::<Vec<_>>();
                        let stereo_samples = samples
                            .chunks_exact(2)
                            .map(|chunk| (chunk[0], chunk[1]))
                            .collect::<Vec<_>>();
                        SampleData::Stereo(stereo_samples)
                    }
                    (2, hound::SampleFormat::Int) => {
                        let samples = reader
                            .samples::<i16>()
                            .map(|s| s.unwrap())
                            .collect::<Vec<_>>();
                        let stereo_samples = samples
                            .chunks_exact(2)
                            .map(|chunk| (chunk[0], chunk[1]))
                            .collect::<Vec<_>>();
                        SampleData::Stereo(stereo_samples)
                    }
                    _ => return None,
                };

                let sample = Sample::new(sample_rate as u32, sample_data, None);
                Some((key, sample))
            })
            .collect();

        for (key, sample) in samples_vec {
            samples_map.insert(key, sample);
        }
    } else {
        // Precalculate piano samples
        let samples_vec: Vec<(u8, Sample)> = (0u8..128)
            .into_par_iter()
            .filter_map(|key| {
                // Exclude drum notes from piano samples
                if key == 36 || key == 38 || key == 42 || key == 46 {
                    // Assuming these are drum notes
                    None
                } else {
                    let freq = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
                    let piano_sample_count = (sample_rate as f32 * 10.0) as usize;
                    let sample_vec = generate_piano_sample(sample_rate, freq, piano_sample_count);
                    let ksynth_sample_data = SampleData::Mono(sample_vec);
                    let ksynth_sample = Sample::new(sample_rate as u32, ksynth_sample_data, None);
                    Some((key, ksynth_sample))
                }
            })
            .collect();

        for (key, sample) in samples_vec {
            samples_map.insert(key, sample);
        }

        // Precalculate drum samples for DrumKit
        let mut drum_kit_map: HashMap<u8, Sample> = HashMap::new();
        let drum_sample_count = (sample_rate as f32 * 2.0) as usize; // Default sample count for drums

        // MIDI GS Drum Map
        let drum_notes = [
            35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
            57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78,
            79, 80, 81, 82, 83, 84,
        ];

        for &key in &drum_notes {
            let sample_vec: Vec<i16> = match key {
                35 => generate_acoustic_bass_drum_sample(sample_rate, drum_sample_count),
                36 => generate_kick_sample(sample_rate, drum_sample_count),
                37 => generate_side_stick_sample(sample_rate, drum_sample_count / 2), // Side stick is short
                38 => generate_snare_sample(sample_rate, drum_sample_count),
                39 => generate_hand_clap_sample(sample_rate, drum_sample_count / 2), // Hand clap is short
                40 => generate_electric_snare_sample(sample_rate, drum_sample_count),
                41 => generate_kick_sample(sample_rate, drum_sample_count), // Low Floor Tom (using kick for now)
                42 => generate_hihat_sample(sample_rate, drum_sample_count / 2), // Closed Hi-Hat
                43 => generate_kick_sample(sample_rate, drum_sample_count), // High Floor Tom (using kick for now)
                44 => generate_pedal_hihat_sample(sample_rate, drum_sample_count / 2), // Pedal Hi-Hat
                45 => generate_kick_sample(sample_rate, drum_sample_count), // Low Tom (using kick for now)
                46 => generate_hihat_sample(sample_rate, drum_sample_count), // Open Hi-Hat
                47 => generate_kick_sample(sample_rate, drum_sample_count), // Low-Mid Tom (using kick for now)
                48 => generate_kick_sample(sample_rate, drum_sample_count), // High-Mid Tom (using kick for now)
                49 => generate_crash_cymbal_sample(sample_rate, drum_sample_count * 2), // Crash Cymbal (longer)
                50 => generate_kick_sample(sample_rate, drum_sample_count), // High Tom (using kick for now)
                51 => generate_ride_cymbal_sample(sample_rate, drum_sample_count * 3), // Ride Cymbal (longer)
                // These will need proper implementation later.
                _ => Vec::new()
            };
            let ksynth_sample_data = SampleData::Mono(sample_vec);
            let ksynth_sample = Sample::new(sample_rate as u32, ksynth_sample_data, None);
            drum_kit_map.insert(key, ksynth_sample);
        }

        drum_kit = Some(DrumKit::new(drum_kit_map));
    }

    if !headless {
        println!("Sample Loaded!");
        println!("Creating KSynth...");
    } else {
        eprintln!("sample_loaded");
        eprintln!("creating_ksynth");
    }

    let samples_arc = Arc::new(RwLock::new(samples_map));
    let mut multi_synth = MultiSynth::new(
        sample_rate,
        ksynth_num_channel,
        max_polyphony as u32,
        ((sample_rate as f64) * 0.1) as u64,
        samples_arc,
        drum_kit,
        if use_multithread { thread_count } else { 1 },
    );
    if !headless {
        println!("KSynth Ready!");
    } else {
        eprintln!("ksynth_ready");
    }

    // MIDIファイルのパスを取得（引数で指定されていない場合はファイルダイアログを表示）
    let midi_path = match args.midi_file_path {
        Some(path) => {
            // パスが存在するか確認
            if !std::path::Path::new(&path).exists() {
                if headless {
                    eprintln!("error MIDI file not found: {}", path);
                    std::process::exit(1);
                } else {
                    eprintln!("Error: MIDI file not found: {}", path);
                    std::process::exit(1);
                }
            }
            path
        }
        None => {
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
        eprintln!("loading_midi_file={}", midi_file_name);
    } else {
        println!("Loading MIDI: {}", midi_file_name);
    }
    let midi = MIDIFile::open(midi_path, None).expect("Failed to open midi file!");
    if headless {
        eprintln!("midi_loaded");
    } else {
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
    } else {
        eprintln!("calculating_midi_statistics");
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

    if !headless {
        println!("Calculated MIDI Statistics");
    } else {
        eprintln!("calculated_midi_statistics");
    }

    let total_frames = (midi_duration.as_secs_f64() * sample_rate as f64).ceil() as u64;
    if headless {
        eprintln!("midi_duration_sec={:.2}", midi_duration.as_secs_f64());
        eprintln!("note_count={}", note_count);
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

    let mut writer = if headless {
        None
    } else {
        Some(
            hound::WavWriter::create(format!("{}.wav", midi_file_name_without_extension), spec)
                .unwrap(),
        )
    };

    let stdout = if headless {
        Some(std::io::stdout())
    } else {
        None
    };
    let mut stdout_lock = stdout.as_ref().map(|s| s.lock());

    if !headless {
        println!("Audio Encoder Created!");
        println!("Rendering Started");
    } else {
        eprintln!("rendering_started")
    }

    let rendering_start_time = Instant::now();

    let mut time_acc = 0.0;
    let mut headless_last_report_time = Instant::now();
    let headless_report_interval = Duration::from_millis(args.log_interval_ms);
    let mut total_rendered_frames: u64 = 0;
    let mut actual_rendered_frames: u64 = 0;

    for merged_event in merge_midi() {
        time_acc += merged_event.delta * sample_rate as f64;

        let frame_count = time_acc.floor() as usize;
        time_acc -= frame_count as f64;

        if frame_count > 0 {
            let mut synth_buffer = vec![0.0f32; frame_count * num_channel as usize];
            multi_synth.fill_buffer(synth_buffer.as_mut_slice());

            if earrape_noise_mode {
                for frame in synth_buffer.chunks_exact_mut(num_channel as usize) {
                    for sample_f32 in frame.iter_mut() {
                        let scaled = (*sample_f32 * 32768.0) as i32;

                        let wrapped = (scaled as u16) as i16;

                        *sample_f32 = (wrapped as f32) / 32768.0;
                    }
                }
            }

            if let Some(ref mut limiters) = limiters {
                for (i, limiter) in limiters.iter_mut().enumerate() {
                    let channel_samples = &mut synth_buffer[i..];
                    limiter.process(channel_samples);
                }
            }

            for frame in synth_buffer.chunks_exact(num_channel as usize) {
                for &sample in frame {
                    if let Some(ref mut w) = writer {
                        w.write_sample(sample).expect("Failed to write sample!");
                    } else if let Some(ref mut out) = stdout_lock {
                        use std::io::Write;
                        out.write_all(&sample.to_le_bytes())
                            .expect("Failed to write PCM!");
                        out.flush().expect("Failed to flush!");
                    }
                }
            }

            if let Some(ref pb) = pb {
                pb.inc(frame_count as u64);
            }
            total_rendered_frames += frame_count as u64;
            actual_rendered_frames += frame_count as u64;
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

        if max_render_speed > 0.0 {
            let expected_elapsed = Duration::from_secs_f64(
                actual_rendered_frames as f64 / (sample_rate as f64 * max_render_speed),
            );
            let actual_elapsed = rendering_start_time.elapsed();

            if actual_elapsed < expected_elapsed {
                std::thread::sleep(expected_elapsed - actual_elapsed);
            }
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
            eprintln!(
                "progress current_sec={:.2} total_sec={:.2} percent={:.1} active_voices={} max_voices={} peak_voices={} rt_percent={:.2}",
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

    if earrape_noise_mode {
        for frame in synth_buffer.chunks_exact_mut(num_channel as usize) {
            for sample_f32 in frame.iter_mut() {
                let scaled = (*sample_f32 * 32768.0) as i32;

                let wrapped = (scaled as u16) as i16;

                *sample_f32 = (wrapped as f32) / 32768.0;
            }
        }
    }

    if let Some(ref mut limiters) = limiters {
        for (i, limiter) in limiters.iter_mut().enumerate() {
            let channel_samples = &mut synth_buffer[i..];
            limiter.process(channel_samples);
        }
    }

    for frame in synth_buffer.chunks_exact(num_channel as usize) {
        for &sample in frame {
            if let Some(ref mut w) = writer {
                w.write_sample(sample).expect("Failed to write sample!");
            } else if let Some(ref mut out) = stdout_lock {
                use std::io::Write;
                out.write_all(&sample.to_le_bytes())
                    .expect("Failed to write PCM!");
                out.flush().expect("Failed to flush!");
            }
        }
    }

    if let Some(w) = writer {
        w.finalize().expect("Failed to finalize!");
    }

    let rendering_end_time = Instant::now();

    let rendering_took_time = rendering_end_time.duration_since(rendering_start_time);

    if let Some(pb) = pb {
        pb.finish();
    } else if headless {
        // Final progress line
        eprintln!(
            "progress current_sec={:.2} total_sec={:.2} percent=100.0 active_voices=0 max_voices={} peak_voices={} rt_percent=0.00",
            midi_duration.as_secs_f64(),
            midi_duration.as_secs_f64(),
            max_polyphony,
            peak_polyphony
        );
    }
    if headless {
        eprintln!("rendering_finished");
        eprintln!(
            "rendering_time_sec={:.2}",
            rendering_took_time.as_secs_f64()
        );
        eprintln!(
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
