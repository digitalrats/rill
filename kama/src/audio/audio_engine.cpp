#include "audio_engine.hpp"
#include "../effects/tape_looper.hpp"
#include "../control/lfo/lfo_robot.hpp"
#include "../control/presets/live_slot_manager.hpp"
#include "../control/signal_router.hpp"
#include "../control/modifiers/modifier_system.hpp"
#include "pipewire_manager.hpp"
#include "../control/osc/osc_handler.hpp"
#include "../control/interfaces/midi_handler.hpp"
#include "../hosting/lv2_host.hpp"
#include <iostream>
#include <cstring>
#include <iomanip>

namespace kama {

AudioEngine::AudioEngine() {
    // Default initialization
}

AudioEngine::~AudioEngine() {
    #if KAMA_BUILD_STANDALONE
    stop();
    #endif
}

std::string AudioEngine::get_version_string() {
    return "0.1.0 (Digital Rats Edition)";
}

std::string AudioEngine::get_organization_string() {
    return "Digital Rats Open-Source Initiative";
}

void AudioEngine::print_digital_rats_banner() {
    #if DIGITAL_RATS_BRANDING
    std::cout << R"(
    ╔═══════════════════════════════════════════════════════╗
    ║                                                       ║
    ║      ██████╗ ██╗ ██████╗ ██╗████████╗ █████╗ ██╗     ║
    ║      ██╔══██╗██║██╔════╝ ██║╚══██╔══╝██╔══██╗██║     ║
    ║      ██║  ██║██║██║  ███╗██║   ██║   ███████║██║     ║
    ║      ██║  ██║██║██║   ██║██║   ██║   ██╔══██║██║     ║
    ║      ██████╔╝██║╚██████╔╝██║   ██║   ██║  ██║███████╗║
    ║      ╚═════╝ ╚═╝ ╚═════╝ ╚═╝   ╚═╝   ╚═╝  ╚═╝╚══════╝║
    ║                                                       ║
    ║      ██████╗  █████╗ ████████╗███████╗                ║
    ║      ██╔══██╗██╔══██╗╚══██╔══╝██╔════╝                ║
    ║      ██████╔╝███████║   ██║   ███████╗                ║
    ║      ██╔══██╗██╔══██║   ██║   ╚════██║                ║
    ║      ██║  ██║██║  ██║   ██║   ███████║                ║
    ║      ╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝   ╚══════╝                ║
    ║                                                       ║
    ║     Open-Source Audio Tools • Creative Coding •       ║
    ║                                                       ║
    ╚═══════════════════════════════════════════════════════╝
    )" << std::endl;
    #endif
}

bool AudioEngine::initialize() {
    try {
        #if DIGITAL_RATS_BRANDING
        print_digital_rats_banner();
        #endif
        
        std::cout << "========================================" << std::endl;
        std::cout << "Initializing Kama Audio Engine v" << get_version_string() << std::endl;
        std::cout << get_organization_string() << std::endl;
        std::cout << "========================================" << std::endl;
        
        std::cout << "Build features:" << std::endl;
        std::cout << "  SIMD: " << KAMA_FEATURE_SIMD << std::endl;
        std::cout << "  WDF: " << KAMA_FEATURE_WDF << std::endl;
        std::cout << "  LV2 Host: " << KAMA_FEATURE_LV2_HOST << std::endl;
        std::cout << "  OSC: " << KAMA_FEATURE_OSC << std::endl;
        std::cout << "  MIDI: " << KAMA_FEATURE_MIDI << std::endl;
        std::cout << "  GUI: " << KAMA_FEATURE_GUI << std::endl;
        std::cout << "  Digital Rats: " << KAMA_FEATURE_DIGITAL_RATS << std::endl;
        
        #if DIGITAL_RATS_BRANDING && ENABLE_RAT_MODE
        std::cout << "  🐀 Rat Mode: Available (secret feature)" << std::endl;
        #endif
        
        // Initialize TapeLooper (core component)
        tape_looper_ = std::make_unique<TapeLooper>(sample_rate_);
        
        #if KAMA_BUILD_STANDALONE
        std::cout << "\nInitializing standalone components..." << std::endl;
        
        #if ENABLE_PIPEWIRE
        // Initialize PipeWire audio backend
        audio_backend_ = std::make_unique<PipeWireManager>();
        struct PipeWireManager::AudioConfig config;
        config.sample_rate = sample_rate_;
        config.channels = 2;
        config.buffer_size = block_size_;
        
        if (!audio_backend_->initialize(config)) {
            std::cerr << "Failed to initialize PipeWire" << std::endl;
            return false;
        }
        
        // Set audio processing callback
        audio_backend_->set_process_callback([this](float* in, float* out, size_t frames) {
            process(in, out, frames);
        });
        #endif // ENABLE_PIPEWIRE
        
        #if ENABLE_OSC
        // Initialize OSC server
        osc_handler_ = std::make_unique<OSCHandler>(*this);
        if (!osc_handler_->start(7777)) {
            std::cerr << "Failed to start OSC server" << std::endl;
        } else {
            std::cout << "📡 OSC server started on port 7777" << std::endl;
            #if DIGITAL_RATS_BRANDING
            std::cout << "   Secret commands: /digitalrats/*" << std::endl;
            #endif
        }
        #endif // ENABLE_OSC
        
        #if ENABLE_MIDI
        // Initialize MIDI handler
        midi_handler_ = std::make_unique<MIDIHandler>(*this);
        if (!midi_handler_->start()) {
            std::cerr << "Failed to start MIDI handler" << std::endl;
        }
        #endif // ENABLE_MIDI
        
        #if ENABLE_LV2_HOST
        // Initialize LV2 host for loading external plugins
        lv2_host_ = std::make_unique<LV2Host>(sample_rate_);
        lv2_host_->scan_plugins();
        #endif // ENABLE_LV2_HOST
        
        #endif // KAMA_BUILD_STANDALONE
        
        #if ENABLE_LFO_ROBOT
        // Initialize LFO Robot automation system
        lfo_robot_ = std::make_unique<LFORobot>(sample_rate_);
        #endif
        
        #if ENABLE_PRESET_SYSTEM
        // Initialize preset/slot system
        slot_manager_ = std::make_unique<LiveSlotManager>();
        signal_router_ = std::make_unique<SignalRouter>(*slot_manager_);
        
        #if KAMA_BUILD_STANDALONE
        // Setup signal connections only for standalone
        setup_signal_connections();
        #endif
        
        #endif
        
        #if ENABLE_MIDI && KAMA_BUILD_STANDALONE
        // Initialize modifier system for MIDI control
        if (midi_handler_ && slot_manager_) {
            modifier_system_ = std::make_unique<ModifierSystem>(*midi_handler_);
        }
        #endif
        
        std::cout << "\n✅ AudioEngine initialized successfully" << std::endl;
        
        #if DIGITAL_RATS_BRANDING
        std::cout << "\n💡 Tip: Try 'rat mode' for experimental features!" << std::endl;
        std::cout << "   OSC: /digitalrats/rat 1" << std::endl;
        #endif
        
        return true;
        
    } catch (const std::exception& e) {
        std::cerr << "💥 Failed to initialize AudioEngine: " << e.what() << std::endl;
        return false;
    }
}

#if KAMA_BUILD_STANDALONE
void AudioEngine::start() {
    if (running_) return;
    
    #if ENABLE_PIPEWIRE
    if (audio_backend_ && audio_backend_->start()) {
        running_ = true;
        std::cout << "🚀 AudioEngine started" << std::endl;
    }
    #else
    running_ = true;
    std::cout << "🚀 AudioEngine started (no audio backend)" << std::endl;
    #endif
}

void AudioEngine::stop() {
    if (!running_) return;
    
    running_ = false;
    
    #if ENABLE_PIPEWIRE
    if (audio_backend_) {
        audio_backend_->stop();
    }
    #endif
    
    std::cout << "🛑 AudioEngine stopped" << std::endl;
}

void AudioEngine::enable_rat_mode(bool enable) {
    #if DIGITAL_RATS_BRANDING && ENABLE_RAT_MODE
    rat_mode_ = enable;
    std::cout << (enable ? "🐀 Rat Mode: ACTIVATED!" : "🐀 Rat Mode: Deactivated") << std::endl;
    
    if (enable) {
        // Enable secret features
        if (tape_looper_) {
            // Extend parameter ranges in rat mode
            std::cout << "   Secret features unlocked!" << std::endl;
            std::cout << "   - Extended feedback range" << std::endl;
            std::cout << "   - Ultra-fast tape speeds" << std::endl;
            std::cout << "   - Hidden filter modes" << std::endl;
        }
    }
    #else
    std::cout << "⚠️  Rat Mode not available in this build" << std::endl;
    #endif
}

#if DIGITAL_RATS_BRANDING
void AudioEngine::secret_cheese_detector(float threshold) {
    std::cout << "🧀 Cheese Detector activated (threshold: " << threshold << ")" << std::endl;
    std::cout << "   Scanning for cheesy frequencies..." << std::endl;
    // This is a playful easter egg feature
}
#endif

#endif // KAMA_BUILD_STANDALONE

void AudioEngine::process(float* input, float* output, size_t frames) {
    std::lock_guard<std::mutex> lock(engine_mutex_);
    
    if (!tape_looper_) {
        // Pass-through if looper not initialized
        if (input != output) {
            memcpy(output, input, frames * 2 * sizeof(float));
        }
        return;
    }
    
    // Apply rat mode effects if enabled
    #if DIGITAL_RATS_BRANDING && ENABLE_RAT_MODE
    if (rat_mode_) {
        // Add subtle randomization in rat mode
        static std::random_device rd;
        static std::mt19937 gen(rd());
        static std::uniform_real_distribution<> dis(-0.01, 0.01);
        
        for (size_t i = 0; i < frames * 2; ++i) {
            input[i] += dis(gen);
        }
    }
    #endif
    
    // Process through tape looper
    tape_looper_->process(input, output, frames);
}

#if KAMA_BUILD_LV2
void AudioEngine::set_sample_rate(double rate) {
    std::lock_guard<std::mutex> lock(engine_mutex_);
    sample_rate_ = static_cast<uint32_t>(rate);
    
    // Reinitialize components with new sample rate
    if (tape_looper_) {
        // TODO: Reinitialize tape looper with new sample rate
    }
}

void AudioEngine::set_block_size(uint32_t size) {
    block_size_ = size;
}
#endif // KAMA_BUILD_LV2

void AudioEngine::setup_signal_connections() {
    #if ENABLE_PRESET_SYSTEM
    if (!signal_router_) return;
    
    // Register control signals with router
    signal_router_->register_signal("tape", "freeze", freeze_changed);
    signal_router_->register_signal("tape", "active_heads", active_heads_changed);
    signal_router_->register_signal("tape", "head_spacing", head_spacing_changed);
    signal_router_->register_signal("tape", "speed", speed_changed);
    signal_router_->register_signal("tape", "feedback", feedback_changed);
    signal_router_->register_signal("tape", "secondary_level", secondary_level_changed);
    signal_router_->register_signal("tape", "master_volume", master_volume_changed);
    signal_router_->register_signal("tape", "mix", mix_changed);
    signal_router_->register_signal("modifier", "shift", shift_modifier_changed);
    
    // Connect signals to tape looper actions
    freeze_changed.connect([this](bool freeze) {
        std::lock_guard<std::mutex> lock(engine_mutex_);
        if (tape_looper_) {
            tape_looper_->set_freeze(freeze);
        }
    });
    
    active_heads_changed.connect([this](int heads) {
        std::lock_guard<std::mutex> lock(engine_mutex_);
        if (tape_looper_) {
            tape_looper_->set_active_heads(heads);
        }
    });
    
    // ... other signal connections
    
    #endif // ENABLE_PRESET_SYSTEM
}

TapeLooper& AudioEngine::get_tape_looper() {
    if (!tape_looper_) {
        throw std::runtime_error("TapeLooper not initialized");
    }
    return *tape_looper_;
}

#if ENABLE_LFO_ROBOT
LFORobot& AudioEngine::get_lfo_robot() {
    if (!lfo_robot_) {
        throw std::runtime_error("LFORobot not initialized");
    }
    return *lfo_robot_;
}
#endif

#if ENABLE_PRESET_SYSTEM
LiveSlotManager& AudioEngine::get_slot_manager() {
    if (!slot_manager_) {
        throw std::runtime_error("LiveSlotManager not initialized");
    }
    return *slot_manager_;
}
#endif

#if ENABLE_LV2_HOST
void AudioEngine::scan_lv2_plugins() {
    if (lv2_host_) {
        lv2_host_->scan_plugins();
    }
}

std::vector<std::string> AudioEngine::get_lv2_plugin_list() const {
    if (lv2_host_) {
        return lv2_host_->get_plugin_list();
    }
    return {};
}
#endif

} // namespace kama
