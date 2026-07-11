#pragma once

// Kama Configuration Header
// Digital Rats Open-Source Project
// Auto-generated from CMake

// Organization info
#define DIGITAL_RATS_ORGANIZATION 1
#define ORGANIZATION_NAME "digitalrats"
#define PROJECT_NAME "kama"
#define PROJECT_FULL_NAME "digitalrats/kama"

// Build type detection
#if defined(KAMA_STANDALONE_BUILD)
    #define KAMA_BUILD_STANDALONE 1
    #define KAMA_BUILD_LV2 0
#elif defined(KAMA_LV2_BUILD)
    #define KAMA_BUILD_STANDALONE 0
    #define KAMA_BUILD_LV2 1
#else
    #define KAMA_BUILD_STANDALONE 1
    #define KAMA_BUILD_LV2 0
#endif

// Feature macros (set by CMake)
#ifndef ENABLE_SIMD
    #define ENABLE_SIMD 0
#endif

#ifndef ENABLE_WDF_FILTERS
    #define ENABLE_WDF_FILTERS 0
#endif

#ifndef ENABLE_LV2_HOST
    #define ENABLE_LV2_HOST 0
#endif

#ifndef ENABLE_GTK_GUI
    #define ENABLE_GTK_GUI 0
#endif

#ifndef ENABLE_OSC
    #define ENABLE_OSC 0
#endif

#ifndef ENABLE_MIDI
    #define ENABLE_MIDI 0
#endif

#ifndef ENABLE_LFO_ROBOT
    #define ENABLE_LFO_ROBOT 0
#endif

#ifndef ENABLE_PRESET_SYSTEM
    #define ENABLE_PRESET_SYSTEM 0
#endif

#ifndef DIGITAL_RATS_BRANDING
    #define DIGITAL_RATS_BRANDING 0
#endif

// Convenience macros
#define KAMA_FEATURE_SIMD ENABLE_SIMD
#define KAMA_FEATURE_WDF ENABLE_WDF_FILTERS
#define KAMA_FEATURE_LV2_HOST ENABLE_LV2_HOST
#define KAMA_FEATURE_GUI ENABLE_GTK_GUI
#define KAMA_FEATURE_OSC ENABLE_OSC
#define KAMA_FEATURE_MIDI ENABLE_MIDI
#define KAMA_FEATURE_LFO_ROBOT ENABLE_LFO_ROBOT
#define KAMA_FEATURE_PRESETS ENABLE_PRESET_SYSTEM
#define KAMA_FEATURE_DIGITAL_RATS DIGITAL_RATS_BRANDING

// Platform detection
#if defined(__arm__) || defined(__aarch64__) || defined(__ARM_ARCH)
    #define KAMA_PLATFORM_ARM 1
    #if defined(__aarch64__) || defined(__ARM_64BIT_STATE)
        #define KAMA_PLATFORM_AARCH64 1
    #else
        #define KAMA_PLATFORM_ARM32 1
    #endif
#elif defined(__i386__) || defined(__x86_64__)
    #define KAMA_PLATFORM_X86 1
    #if defined(__x86_64__)
        #define KAMA_PLATFORM_X64 1
    #endif
#elif defined(__riscv)
    #define KAMA_PLATFORM_RISCV 1
#endif

// SIMD alignment
#if KAMA_FEATURE_SIMD
    #if KAMA_PLATFORM_X64 || KAMA_PLATFORM_AARCH64
        #define KAMA_ALIGN alignas(32)
    #else
        #define KAMA_ALIGN alignas(16)
    #endif
#else
    #define KAMA_ALIGN
#endif

// Buffer sizes
#if KAMA_PLATFORM_ARM32
    #define DEFAULT_BUFFER_SIZE 128
#else
    #define DEFAULT_BUFFER_SIZE 256
#endif

// Conditional compilation helpers
#if KAMA_BUILD_STANDALONE
    #define KAMA_IF_STANDALONE(...) __VA_ARGS__
    #define KAMA_IF_LV2(...)
#else
    #define KAMA_IF_STANDALONE(...)
    #define KAMA_IF_LV2(...) __VA_ARGS__
#endif

#if defined(KAMA_WITH_GUI)
    #define KAMA_GUI_MODE 1
#else
    #define KAMA_GUI_MODE 0
#endif

#if defined(KAMA_CLI_MODE)
    #define KAMA_CLI_MODE 1
#else
    #define KAMA_CLI_MODE 0
#endif

// Digital Rats easter eggs
#if DIGITAL_RATS_BRANDING
    #define DIGITAL_RATS_EASTER_EGG 1
    
    // Hidden features
    #define ENABLE_RAT_MODE 1
    #define ENABLE_CHEESE_DETECTOR 0  // Experimental
    
    // Secret parameter ranges
    #define SECRET_FEEDBACK_MAX 2.0f
    #define SECRET_SPEED_MAX 8.0f
    
    // Hidden OSC commands
    #define OSC_CMD_RAT "/digitalrats/rat"
    #define OSC_CMD_CHEESE "/digitalrats/cheese"
    
#else
    #define DIGITAL_RATS_EASTER_EGG 0
    #define ENABLE_RAT_MODE 0
#endif
