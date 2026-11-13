//! Constructors of the EXR II devices the interface crates still hold.
use super::{Builder, Built, Constructors, decode, line};
use crate::Model;
use fujimod_emulator_devices::exr2::{clock, gpio, irq, pulse};
use fujimod_emulator_exr2_resources::CoreInterrupts;

/// Add the constructors of the devices the interface crates hold.
pub fn register_interface_models(constructors: &mut Constructors) {
    constructors.add("clocks", clocks);
    constructors.add("exr2-interrupt-controller", interrupt_controller);
    constructors.add("gpio", gpio);
    constructors.add("exr2-pulse", pulse_unit);
    constructors.add("block86000", block86000);
    constructors.add("register-stub", register_stub);
    constructors.add("sata-host", sata_host);
    constructors.add("exr-adc", exr_adc);
    constructors.add("exr-command-gpu", exr_command_gpu);
    constructors.add("exr-display-pipe", exr_display_pipe);
    constructors.add("hdmi-sink", hdmi_sink);
    constructors.add("exr-system-control", exr_system_control);
    constructors.add("exr-clocks", exr_clocks);
    constructors.add("exr-memory-interface", exr_memory_interface);
    constructors.add("sxc-intc", sxc_intc);
    constructors.add("sxc-timers", sxc_timers);
    constructors.add("sxc-usb-ctrl", sxc_usb_ctrl);
    constructors.add("sxc-dma-channel", sxc_dma_channel);
}

fn sxc_intc(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Builder { id, built, .. } = builder;
    let Model::SxcIntc { core, base } = model else {
        return Err("constructor called for another model kind".into());
    };
    if core >= 2 || built.core_interrupts.iter().any(|c| c.core == core) {
        return Err("invalid or duplicate SXC core interrupt controller".into());
    }
    let inputs = fujimod_emulator_devices::sxc::intc::Inputs::default();
    let controller = fujimod_emulator_devices::sxc::intc::InterruptController::new(base, inputs.clone());
    built.core_interrupts.push(CoreInterrupts {
        core,
        device: id,
        inputs: Box::new(inputs),
        exr2_inputs: None,
    });
    Ok(Built::Device(Box::new(controller)))
}

fn sxc_timers(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::SxcTimers { base } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::sxc::timer::Timers::new(base),
    )))
}

fn sxc_usb_ctrl(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::SxcUsbCtrl { base } = model else {
        return Err("constructor called for another model kind".into());
    };
    let nand = builder
        .resources
        .sxc_nand
        .get_or_insert_with(Default::default)
        .clone();
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::sxc::usb_ctrl::UsbController::with_nand(base, nand),
    )))
}

fn sxc_dma_channel(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::SxcDmaChannel {
        base,
        core,
        irq,
        ram_base,
    } = model
    else {
        return Err("constructor called for another model kind".into());
    };
    let nand = builder
        .resources
        .sxc_nand
        .get_or_insert_with(Default::default)
        .clone();
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::sxc::dma::DmaChannel::with_nand(base, core, irq, ram_base, nand),
    )))
}

fn exr_memory_interface(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    if !matches!(model, Model::ExrMemoryInterface) {
        return Err("constructor called for another model kind".into());
    }
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr::MemoryInterface,
    )))
}

fn exr_clocks(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::ExrClocks { options } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr::clocks::Clocks::new(decode(options)?)?,
    )))
}

fn exr_system_control(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::ExrSystemControl { options, wiring } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr::SystemControl::new(decode(options)?, decode(wiring)?)?,
    )))
}

fn clocks(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Builder {
        name, resources, ..
    } = builder;
    let Model::Clocks { options } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device({
        let clock = clock::Clocks::new(options, ())?;
        resources.clocks.insert(name.into(), clock.output());
        Box::new(clock)
    }))
}

fn interrupt_controller(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Builder {
        name,
        id,
        resources,
        built,
        ..
    } = builder;
    let Model::InterruptController {
        options,
        core,
        base,
        routes,
        software,
    } = model
    else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device({
        if core >= 2 || built.core_interrupts.iter().any(|c| c.core == core) {
            return Err("invalid or duplicate EXR II core interrupt controller".into());
        }
        let enables = routes
            .map(|name| {
                let routes = fujimod_emulator_exr2_resources::wire!(resources, id, name, IrqRoutes);
                Ok(Box::new(
                    fujimod_emulator_devices::exr2::irq_routes::RouteOutput::new(routes, core)?,
                ) as Box<dyn irq::EnableOutput>)
            })
            .transpose()?;
        let inputs = irq::Inputs::default();
        resources.controllers.insert(name.into(), inputs.clone());
        let controller = irq::InterruptController::new(
            options,
            irq::Wiring {
                base,
                inputs: inputs.clone(),
                enables,
                software: software
                    .into_iter()
                    .map(|output| {
                        if !resources.software_sources.insert(output.pulse.clone()) {
                            return Err("software pulse requires one controller output".into());
                        }
                        Ok(irq::SoftwareOutput {
                            line: output.line,
                            pulse: fujimod_emulator_exr2_resources::wire!(
                                resources,
                                id,
                                output.pulse,
                                SoftwarePulse
                            ),
                        })
                    })
                    .collect::<Result<_, String>>()?,
            },
        )?;
        built.core_interrupts.push(CoreInterrupts {
            core,
            device: id,
            inputs: Box::new(inputs.clone()),
            exr2_inputs: Some(inputs),
        });
        Box::new(controller)
    }))
}

fn gpio(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Builder {
        name, resources, ..
    } = builder;
    let Model::Gpio { options, irqs } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device({
        for route in &irqs {
            for &output in &route.outputs {
                line(output)?;
            }
        }
        let gpio = gpio::Gpio::new(options, gpio::Wiring { irqs })?;
        resources.gpio.insert(name.to_owned(), gpio.pads());
        Box::new(gpio)
    }))
}

fn pulse_unit(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Builder {
        name, resources, ..
    } = builder;
    let Model::PulseUnit { options, irqs } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device({
        for irq in irqs.into_iter().flatten() {
            line(irq)?;
        }
        let pulse = pulse::PulseUnit::new(options, pulse::Wiring { irqs })?;
        resources.pulses.insert(name.to_owned(), pulse.pins());
        Box::new(pulse)
    }))
}

fn block86000(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::Block86000 { options } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device(Box::new(pulse::block86000::Block86000::new(
        options,
    ))))
}

fn register_stub(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::RegisterStub { options } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr::register_stub::RegisterStub::new(options)?,
    )))
}

fn sata_host(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::SataHost { options } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr2::sata_host::SataHost::new(options)?,
    )))
}

fn exr_adc(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::ExrAdc { options } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr::adc::Adc::new(options)?,
    )))
}

fn exr_command_gpu(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::ExrCommandGpu { options, irq } = model else {
        return Err("constructor called for another model kind".into());
    };
    if let Some(irq) = irq {
        line(irq)?;
    }
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr::command_gpu::CommandGpu::new(options, irq)?,
    )))
}

fn exr_display_pipe(_builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Model::ExrDisplayPipe {
        options,
        irq,
        layer_irq,
    } = model
    else {
        return Err("constructor called for another model kind".into());
    };
    for irq in [irq, layer_irq].into_iter().flatten() {
        line(irq)?;
    }
    Ok(Built::Device(Box::new(
        fujimod_emulator_devices::exr::display_pipe::DisplayPipe::new(
            options,
            fujimod_emulator_devices::exr::display_pipe::Wiring { irq, layer_irq },
        )?,
    )))
}

fn hdmi_sink(builder: Builder<'_>, model: Model) -> Result<Built, String> {
    let Builder {
        name, resources, ..
    } = builder;
    let Model::HdmiSink { options } = model else {
        return Err("constructor called for another model kind".into());
    };
    Ok(Built::Device({
        let options: fujimod_emulator_devices::parts::hdmi_sink::Options = decode(options)?;
        let input = fujimod_emulator_devices::parts::hdmi_sink::Input::new(options)?;
        resources.hdmi_sinks.insert(name.to_owned(), input.sink());
        Box::new(input)
    }))
}
