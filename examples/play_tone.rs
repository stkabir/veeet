use std::f64::consts::TAU;
use std::time::{Duration, Instant};

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat, initialize_mta};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = initialize_mta();
    let enumerator = DeviceEnumerator::new()?;
    let device = enumerator.get_default_device(Direction::Render)?;
    println!(
        "reproduciendo en: {}",
        device.get_friendlyname().unwrap_or_default()
    );

    let mut client = device.get_iaudioclient()?;
    let channels = 2usize;
    let format = WaveFormat::new(32, 32, SampleType::Float, 48_000, channels, None);
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: 200_000,
    };
    client.initialize_client(&format, Direction::Render, mode, None)?;

    let event = client.set_get_eventhandle()?;
    let render = client.get_audiorenderclient()?;

    let mut time = 0.0f64;
    let mut fill = |frames: usize| {
        let mut samples = vec![0f32; frames * channels];
        for frame in 0..frames {
            let t = time;
            time += 1.0 / 48_000.0;
            let beat = t % 0.5;
            let kick = (-beat * 18.0).exp() * (TAU * 55.0 * t).sin() * 0.85;
            let chord = (TAU * 220.0 * t).sin() * 0.12
                + (TAU * 277.18 * t).sin() * 0.10
                + (TAU * 329.63 * t).sin() * 0.10;
            let melody = (TAU * 440.0 * t).sin() * 0.12;
            let swell = 0.6 + 0.4 * (TAU * 0.2 * t).sin();
            let sample = ((kick + chord + melody) * swell * 0.7) as f32;
            samples[frame * channels] = sample;
            samples[frame * channels + 1] = sample;
        }
        let bytes = bytemuck::cast_slice(&samples);
        render.write_to_device(bytes, None).unwrap();
    };

    let initial = client.get_available_space_in_frames()?;
    fill(initial as usize);
    client.start_stream()?;
    println!("tono iniciado ({} frames iniciales)", initial);

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if event.wait_for_event(1000).is_err() {
            println!("timeout de reproducción");
            break;
        }
        let available = client.get_available_space_in_frames()?;
        if available > 0 {
            fill(available as usize);
        }
    }
    client.stop_stream()?;
    println!("tono terminado");
    Ok(())
}
