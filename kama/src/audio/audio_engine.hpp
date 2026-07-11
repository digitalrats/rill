#pragma once
#include <memory>
#include <mutex>
#include <atomic>
#include <sigslot/signal.hpp>
#include <string>
#include <vector>
#include "../kama/config.hpp"

namespace kama {

// Forward declarations
class PipeWireManager;
class TapeLooper;
class LFORobot;
class LiveSlotManager;
class SignalRouter;
class ModifierSystem;
class OSCHandler;
class MIDIHandler;
class LV2Host;

class AudioEngine {
public:
    AudioEngine();
    ~AudioEngine();
    
    bool initialize();
    
    #if KAMA_BUILD_STANDALONE
    void start();
    void stop();
    bool is_running() const { return running_; }
    
    // Digital Rats specific methods
    void enable_rat_mode(bool enable);
    bool is_rat_mode_enabled() const { return rat_mode_; }
    
    #if DIGITAL_RATS_BRANDING
    void secret_cheese_detector(float threshold);
    #endif
    #endif
    
    void process(float* input, float* output, size_t frames);
    
    #if KAMA_BUILD_LV2
    void set_sample_rate(double rate);
    void set_block_size(uint32_t size);
    #endif
    
    // Control signals (connected to SignalRouter)
    sigslot::signal<bool> freeze_changed;
    sigslot::signal<int> active_heads_changed; // 0-3
    sigslot::signal<float> head_spacing_changed;
    sigslot::signal<float> speed_changed;
    sigslot::signal<float> feedback_changed;
    sigslot::signal<float> secondary_level_changed;
    sigslot::signal<float> master_volume_changed;
    sigslot::signal<float> mix_changed;
    sigslot::signal<bool> shift_modifier_changed;
    
    // Component access
    class TapeLooper& get_tape_looper();
    
    #if ENABLE_LFO_ROBOT
    class LFORobot& get_lfo_robot();
    #endif
    
    #if ENABLE_PRESET_SYSTEM
    class LiveSlotManager& get_slot_manager();
    #endif
    
    #if ENABLE_LV2_HOST
    void scan_lv2_plugins();
    std::vector<std::string> get_lv2_plugin_list() const;
    #endif
    
    // Version info
    static std::string get_version_string();
    static std::string get_organization_string();
    
private:
    void setup_signal_connections();
    void print_digital_rats_banner();
    
    std::unique_ptr<class TapeLooper> tape_looper_;
    
    #if KAMA_BUILD_STANDALONE
    std::unique_ptr<class PipeWireManager> audio_backend_;
    std::atomic<bool> running_{false};
    std::atomic<bool> rat_mode_{false};
    
    #if ENABLE_OSC
    std::unique_ptr<class OSCHandler> osc_handler_;
    #endif
    
    #if ENABLE_MIDI
    std::unique_ptr<class MIDIHandler> midi_handler_;
    #endif
    
    #if ENABLE_LV2_HOST
    std::unique_ptr<class LV2Host> lv2_host_;
    #endif
    #endif // KAMA_BUILD_STANDALONE
    
    #if ENABLE_LFO_ROBOT
    std::unique_ptr<class LFORobot> lfo_robot_;
    #endif
    
    #if ENABLE_PRESET_SYSTEM
    std::unique_ptr<class LiveSlotManager> slot_manager_;
    std::unique_ptr<class SignalRouter> signal_router_;
    #endif
    
    #if ENABLE_MIDI && KAMA_BUILD_STANDALONE
    std::unique_ptr<class ModifierSystem> modifier_system_;
    #endif
    
    std::mutex engine_mutex_;
    uint32_t sample_rate_ = 48000;
    uint32_t block_size_ = 256;
};

} // namespace kama
