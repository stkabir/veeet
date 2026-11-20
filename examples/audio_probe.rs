use std::time::{Duration, Instant};

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat, initialize_mta};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = initialize_mta();
    let enumerator = DeviceEnumerator::new()?;
    let device = enumerator.get_default_device(Direction::Render)?;
    println!(
        "dispositivo: {} / estado {:?}",
        device.get_friendlyname().unwrap_or_default(),
        device.get_state()
    );
    let mut client = device.get_iaudioclient()?;
    let mix = client.get_mixformat()?;
    println!(
        "mix: {} Hz, {} canales, {} bits, block {}",
        mix.get_samplespersec(),
        mix.get_nchannels(),
        mix.get_bitspersample(),
        mix.get_blockalign()
    );

    let desired = WaveFormat::new(32, 32, SampleType::Float, 48_000, 2, None);
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: 200_000,
    };
    client.initialize_client(&desired, Direction::Capture, mode, None)?;
    println!("cliente inicializado");

    let event = client.set_get_eventhandle()?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    println!("stream iniciado");

    let start = Instant::now();
    let mut total_frames = 0usize;
    let mut packets = 0usize;
    let mut peak = 0f32;
    let mut buffer = vec![0f32; 4096 * 2];

    while start.elapsed() < Duration::from_secs(12) {
        if event.wait_for_event(500).is_err() {
            println!("timeout sin eventos");
            continue;
        }
        loop {
            let frames = capture.get_next_packet_size()?.unwrap_or(0) as usize;
            if frames == 0 {
                break;
            }
            if buffer.len() < frames * 2 {
                buffer.resize(frames * 2, 0.0);
            }
            let bytes = bytemuck::cast_slice_mut::<f32, u8>(&mut buffer[..frames * 2]);
            let (read, info) = capture.read_from_device(bytes)?;
            packets += 1;
            total_frames += read as usize;
            for frame in 0..read as usize {
                peak = peak
                    .max(buffer[frame * 2].abs())
                    .max(buffer[frame * 2 + 1].abs());
            }
            if packets <= 3 {
                println!("paquete: {read} frames, silent={}", info.flags.silent);
            }
        }
    }
    client.stop_stream()?;
    println!("paquetes={packets} frames={total_frames} pico={peak:.4}");
    Ok(())
}
