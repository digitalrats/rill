# Kama Build Configuration
# Digital Rats Open-Source Project
# =================================

# Organization and project info
set(ORGANIZATION_NAME "digitalrats")
set(PROJECT_NAME "kama")
set(PROJECT_DESCRIPTION "Modern tape-style looper and effects processor")

# Тип сборки
set(KAMA_BUILD_TYPE "both" CACHE STRING 
    "Build type: standalone (application), lv2 (plugin), both")

# Проверка валидности
set(VALID_BUILD_TYPES standalone lv2 both)
if(NOT KAMA_BUILD_TYPE IN_LIST VALID_BUILD_TYPES)
    message(FATAL_ERROR "Invalid KAMA_BUILD_TYPE: ${KAMA_BUILD_TYPE}. "
                        "Valid options: standalone, lv2, both")
endif()

# Архитектурные опции
option(ENABLE_SIMD "Enable SIMD optimizations" ON)
option(ENABLE_MULTITHREADING "Enable multithreaded processing" ON)
option(ENABLE_REALTIME_PRIORITY "Enable real-time thread priority" OFF)

# Аудио backend
option(ENABLE_PIPEWIRE "Enable PipeWire audio backend" ON)
option(ENABLE_JACK "Enable JACK audio backend" OFF)
option(ENABLE_ALSA "Enable ALSA audio backend" OFF)
option(ENABLE_PULSEAUDIO "Enable PulseAudio backend" OFF)

# Фильтры и DSP
option(ENABLE_WDF_FILTERS "Enable WDF analog modeling filters" ON)
option(ENABLE_DIGITAL_FILTERS "Enable digital biquad filters" ON)
option(ENABLE_ALGORITHMIC_FILTERS "Enable algorithmic filters" ON)
option(ENABLE_FILTER_CHAIN "Enable filter chaining system" ON)

# Эффекты
option(ENABLE_LFO_ROBOT "Enable LFO Robot automation system" ON)
option(ENABLE_TAPE_SATURATION "Enable tape saturation modeling" ON)
option(ENABLE_WOW_FLUTTER "Enable wow & flutter effects" ON)
option(ENABLE_TAPE_NOISE "Enable tape noise generation" ON)

# Пресеты и управление
option(ENABLE_PRESET_SYSTEM "Enable preset/slot system" ON)
option(ENABLE_LIVE_SLOTS "Enable live slot management" ON)
option(ENABLE_FACTORY_PRESETS "Enable factory presets" ON)

# Интерфейсы
option(ENABLE_OSC "Enable OSC remote control" ON)
option(ENABLE_MIDI "Enable MIDI control" ON)
option(ENABLE_HID "Enable HID (keyboard/mouse) control" ON)
option(ENABLE_LEARN_MODE "Enable controller learn mode" ON)

# Расширенные возможности
option(ENABLE_LV2_HOST "Enable LV2 plugin host system" OFF)
option(ENABLE_LV2_PLUGIN "Build LV2 plugin version" OFF)
option(ENABLE_VST3 "Build VST3 plugin (experimental)" OFF)
option(ENABLE_CLAP "Build CLAP plugin (experimental)" OFF)

# GUI
option(ENABLE_GTK_GUI "Enable GTK graphical interface" ON)
option(ENABLE_OPENGL "Enable OpenGL accelerated UI" OFF)
option(ENABLE_TOUCH_SCREEN "Enable touch screen optimizations" OFF)

# Тестирование и отладка
option(ENABLE_TESTS "Build unit tests" OFF)
option(ENABLE_BENCHMARKS "Build performance benchmarks" OFF)
option(ENABLE_DEBUG_SYMBOLS "Include debug symbols in release builds" OFF)
option(ENABLE_SANITIZERS "Enable address/undefined behavior sanitizers" OFF)
option(ENABLE_COVERAGE "Enable code coverage reporting" OFF)

# Целевая платформа
set(KAMA_TARGET_PLATFORM "generic" CACHE STRING "Target platform: generic, arm, armhf, aarch64, x86, x64, riscv, embedded")
set(KAMA_OPTIMIZATION_LEVEL "balanced" CACHE STRING "Optimization level: minimal, balanced, aggressive, maximum")

# Минимальные требования для встраиваемых систем
option(KAMA_MINIMAL_BUILD "Minimal build for embedded systems" OFF)

# Digital Rats branding
option(ENABLE_DIGITAL_RATS_BRANDING "Enable Digital Rats branding and easter eggs" ON)

# Автоматическая настройка зависимостей на основе типа сборки
if(KAMA_BUILD_TYPE STREQUAL "standalone" OR KAMA_BUILD_TYPE STREQUAL "both")
    # Standalone требует аудио бэкенд
    if(NOT ENABLE_PIPEWIRE AND NOT ENABLE_JACK AND NOT ENABLE_ALSA)
        set(ENABLE_PIPEWIRE ON CACHE BOOL "Enable PipeWire for standalone build" FORCE)
    endif()
endif()

if(KAMA_BUILD_TYPE STREQUAL "lv2")
    # LV2 плагин не нуждается в GUI
    set(ENABLE_GTK_GUI OFF CACHE BOOL "GUI not needed for LV2 plugin" FORCE)
    # Но требует LV2 заголовки
    find_package(LV2 QUIET)
    if(NOT LV2_FOUND)
        message(WARNING "LV2 development headers not found. LV2 plugin may not build correctly.")
    endif()
endif()

# Проверка зависимостей
macro(check_dependency DEP_NAME VAR_NAME PKG_NAME)
    find_package(PkgConfig)
    if(PKG_NAME)
        pkg_check_modules(${VAR_NAME} ${PKG_NAME})
        if(${VAR_NAME}_FOUND)
            set(${DEP_NAME}_FOUND TRUE)
            message(STATUS "Found ${DEP_NAME}: ${_LIBRARIES}")
        else()
            set(${DEP_NAME}_FOUND FALSE)
            message(WARNING "${DEP_NAME} not found - ${DEP_NAME} features disabled")
        endif()
    endif()
endmacro()

# Автоматическое определение платформы
if(NOT KAMA_TARGET_PLATFORM)
    if(CMAKE_SYSTEM_PROCESSOR MATCHES "arm" OR CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64")
        if(CMAKE_SIZEOF_VOID_P EQUAL 8)
            set(KAMA_TARGET_PLATFORM "aarch64" CACHE STRING "" FORCE)
        else()
            set(KAMA_TARGET_PLATFORM "armhf" CACHE STRING "" FORCE)
        endif()
    elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "x86_64")
        set(KAMA_TARGET_PLATFORM "x64" CACHE STRING "" FORCE)
    elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "i386" OR CMAKE_SYSTEM_PROCESSOR MATCHES "i686")
        set(KAMA_TARGET_PLATFORM "x86" CACHE STRING "" FORCE)
    elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "riscv")
        set(KAMA_TARGET_PLATFORM "riscv" CACHE STRING "" FORCE)
    else()
        set(KAMA_TARGET_PLATFORM "generic" CACHE STRING "" FORCE)
    endif()
endif()

message(STATUS "Target platform: ${KAMA_TARGET_PLATFORM}")

# Настройки оптимизации
if(KAMA_OPTIMIZATION_LEVEL STREQUAL "minimal")
    add_compile_options(-Os -ffunction-sections -fdata-sections)
    add_link_options(-Wl,--gc-sections)
elseif(KAMA_OPTIMIZATION_LEVEL STREQUAL "balanced")
    add_compile_options(-O2 -ftree-vectorize)
elseif(KAMA_OPTIMIZATION_LEVEL STREQUAL "aggressive")
    add_compile_options(-O3 -march=native -mtune=native -ffast-math)
elseif(KAMA_OPTIMIZATION_LEVEL STREQUAL "maximum")
    add_compile_options(-Ofast -march=native -mtune=native -ffast-math -funroll-loops)
endif()

# Минимальная сборка
if(KAMA_MINIMAL_BUILD)
    set(ENABLE_LV2_HOST OFF CACHE BOOL "" FORCE)
    set(ENABLE_GTK_GUI OFF CACHE BOOL "" FORCE)
    set(ENABLE_OSC OFF CACHE BOOL "" FORCE)
    set(ENABLE_PRESET_SYSTEM ON CACHE BOOL "" FORCE)  # Но упрощенная
    set(ENABLE_LFO_ROBOT OFF CACHE BOOL "" FORCE)
    set(ENABLE_WDF_FILTERS OFF CACHE BOOL "" FORCE)
    set(ENABLE_SIMD OFF CACHE BOOL "" FORCE)
    set(ENABLE_DIGITAL_RATS_BRANDING OFF CACHE BOOL "" FORCE)
    message(STATUS "Minimal build enabled - only essential features")
endif()

# Сообщение о конфигурации
message(STATUS "digitalrats - kama Build Configuration:")
message(STATUS "  Build type: ${KAMA_BUILD_TYPE}")
message(STATUS "  SIMD: ${ENABLE_SIMD}")
message(STATUS "  WDF Filters: ${ENABLE_WDF_FILTERS}")
message(STATUS "  LV2 Host: ${ENABLE_LV2_HOST}")
message(STATUS "  LFO Robot: ${ENABLE_LFO_ROBOT}")
message(STATUS "  GUI: ${ENABLE_GTK_GUI}")
message(STATUS "  PipeWire: ${ENABLE_PIPEWIRE}")
message(STATUS "  Digital Rats Branding: ${ENABLE_DIGITAL_RATS_BRANDING}")
