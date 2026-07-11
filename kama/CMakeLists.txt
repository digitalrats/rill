cmake_minimum_required(VERSION 3.16)
project(kama VERSION 0.1.0 LANGUAGES CXX C)

# Digital Rats Open-Source Project
# =================================
set(ORGANIZATION_NAME "digitalrats")
set(PROJECT_FULL_NAME "digitalrats/kama")
set(PROJECT_DESCRIPTION "Modern tape-style looper and effects processor")
set(PROJECT_HOMEPAGE_URL "https://github.com/digitalrats/kama")
set(PROJECT_LICENSE "GPL-3.0-or-later")

message(STATUS "=========================================================================")
message(STATUS "Building ${PROJECT_FULL_NAME} v${PROJECT_VERSION}")
message(STATUS "Digital Rats Open-Source Initiative")
message(STATUS "=========================================================================")

# Optimized for Ninja build system
if(CMAKE_GENERATOR STREQUAL "Ninja")
    # Ninja может использовать больше параллельных задач
    set(CMAKE_JOB_POOLS compile=16 link=8)
    set(CMAKE_JOB_POOL_COMPILE compile)
    set(CMAKE_JOB_POOL_LINK link)
    
    # Используем colored output для ninja
    if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
        add_compile_options(-fdiagnostics-color=always)
    endif()
    
    message(STATUS "Using Ninja build system with job pool optimization")
endif()

# Устанавливаем политику для лучшей поддержки Ninja
if(POLICY CMP0116)
    cmake_policy(SET CMP0116 NEW)  # Ninja multi-config
endif()
if(POLICY CMP0138)
    cmake_policy(SET CMP0138 NEW)  # MSVC runtime library для Ninja
endif()

# Загружаем опции
include(CMakeOptions.cmake)

# Стандарт C++ (с учетом платформы)
if(KAMA_TARGET_PLATFORM MATCHES "embedded|arm|riscv")
    set(CMAKE_CXX_STANDARD 17)
    set(CMAKE_CXX_STANDARD_REQUIRED ON)
    set(CMAKE_CXX_EXTENSIONS OFF)
else()
    set(CMAKE_CXX_STANDARD 20)
    set(CMAKE_CXX_STANDARD_REQUIRED ON)
    set(CMAKE_CXX_EXTENSIONS OFF)
endif()

# Настройки компилятора
if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    add_compile_options(-Wall -Wextra -Wpedantic)
    
    # Предупреждения для разных платформ
    if(NOT KAMA_TARGET_PLATFORM MATCHES "embedded")
        add_compile_options(-Wshadow -Wconversion -Wsign-conversion)
    endif()
    
    if(ENABLE_DEBUG_SYMBOLS)
        add_compile_options(-g)
    endif()
    
    if(ENABLE_SANITIZERS AND NOT KAMA_MINIMAL_BUILD)
        add_compile_options(-fsanitize=address,undefined)
        add_link_options(-fsanitize=address,undefined)
    endif()
endif()

# Digital Rats branding
if(ENABLE_DIGITAL_RATS_BRANDING)
    add_compile_definitions(DIGITAL_RATS_BRANDING=1)
    message(STATUS "Digital Rats branding enabled")
endif()

# Основная библиотека ядра (общая для всех типов сборки)
add_library(kama_core STATIC
    src/audio/audio_engine.cpp
    src/effects/tape_looper.cpp
    src/effects/filter_chain.cpp
    src/control/presets/preset_manager.cpp
    src/control/presets/live_slot_manager.cpp
    src/control/signal_router.cpp
    src/control/modifiers/modifier_system.cpp
    $<$<BOOL:${ENABLE_WDF_FILTERS}>:src/dsp/wdf_filters.cpp>
    $<$<BOOL:${ENABLE_DIGITAL_FILTERS}>:src/dsp/biquad_filters.cpp>
    $<$<BOOL:${ENABLE_LFO_ROBOT}>:src/control/lfo/lfo_robot.cpp>
)

# Включаем директории
target_include_directories(kama_core PRIVATE
    include
)

# Digital Rats easter egg
if(ENABLE_DIGITAL_RATS_BRANDING)
    target_compile_definitions(kama_core PRIVATE DIGITAL_RATS_EASTER_EGG)
endif()

# SIMD оптимизации
if(ENABLE_SIMD)
    find_package(xsimd QUIET)
    if(xsimd_FOUND)
        target_compile_definitions(kama_core PRIVATE ENABLE_SIMD)
        target_include_directories(kama_core PRIVATE ${xsimd_INCLUDE_DIRS})
        message(STATUS "SIMD optimizations enabled")
    else()
        message(WARNING "xsimd not found - SIMD optimizations disabled")
        set(ENABLE_SIMD OFF CACHE BOOL "" FORCE)
    endif()
endif()

# WDF фильтры
if(ENABLE_WDF_FILTERS)
    # Проверяем наличие ChowDSP
    find_package(ChowDSP QUIET)
    if(ChowDSP_FOUND)
        target_compile_definitions(kama_core PRIVATE ENABLE_WDF_FILTERS)
        target_include_directories(kama_core PRIVATE ${ChowDSP_INCLUDE_DIRS})
        target_link_libraries(kama_core ChowDSP::WDF)
        message(STATUS "WDF filters enabled")
    else()
        # Встроенная минимальная версия WDF
        add_subdirectory(vendor/miniwdf)
        target_link_libraries(kama_core miniwdf)
        message(STATUS "Using built-in minimal WDF implementation")
    endif()
endif()

# Standalone приложение
if(KAMA_BUILD_TYPE STREQUAL "standalone" OR KAMA_BUILD_TYPE STREQUAL "both")
    message(STATUS "Building standalone application")
    
    # Выбираем GUI или CLI версию
    if(ENABLE_GTK_GUI)
        # Проверяем GTK
        find_package(PkgConfig REQUIRED)
        pkg_check_modules(GTKMM REQUIRED gtkmm-4.0)
        
        if(GTKMM_FOUND)
            add_executable(kama-standalone
                src/standalone/main_gui.cpp
                src/gui/main_window.cpp
                $<$<BOOL:${ENABLE_LFO_ROBOT}>:src/gui/lfo_panel.cpp>
                $<$<BOOL:${ENABLE_LV2_HOST}>:src/gui/lv2_chain_editor.cpp>
            )
            
            target_compile_definitions(kama-standalone PRIVATE 
                KAMA_STANDALONE_BUILD
                KAMA_WITH_GUI
            )
            
            target_include_directories(kama-standalone PRIVATE 
                include
                ${GTKMM_INCLUDE_DIRS}
            )
            
            target_link_libraries(kama-standalone 
                kama_core 
                ${GTKMM_LIBRARIES}
            )
            
        else()
            message(WARNING "GTK not found - falling back to CLI")
            set(ENABLE_GTK_GUI OFF CACHE BOOL "" FORCE)
        endif()
    endif()
    
    # Если GUI отключен или не найден - строим CLI
    if(NOT ENABLE_GTK_GUI)
        add_executable(kama-standalone
            src/standalone/main_cli.cpp
            src/cli/controller.cpp
        )
        
        target_compile_definitions(kama-standalone PRIVATE 
            KAMA_STANDALONE_BUILD
            KAMA_CLI_MODE
        )
        
        target_include_directories(kama-standalone PRIVATE include)
        target_link_libraries(kama-standalone kama_core pthread)
    endif()
    
    # PipeWire для standalone
    if(ENABLE_PIPEWIRE)
        find_package(PkgConfig REQUIRED)
        pkg_check_modules(PIPEWIRE REQUIRED libpipewire-0.3)
        
        if(PIPEWIRE_FOUND)
            target_compile_definitions(kama-standalone PRIVATE ENABLE_PIPEWIRE)
            target_include_directories(kama-standalone PRIVATE ${PIPEWIRE_INCLUDE_DIRS})
            target_link_libraries(kama-standalone ${PIPEWIRE_LIBRARIES})
            message(STATUS "PipeWire backend enabled for standalone")
        else()
            message(WARNING "PipeWire not found - audio may not work")
        endif()
    endif()
    
    # OSC поддержка для standalone
    if(ENABLE_OSC)
        find_package(PkgConfig REQUIRED)
        pkg_check_modules(LIBLO REQUIRED liblo)
        
        if(LIBLO_FOUND)
            target_compile_definitions(kama-standalone PRIVATE ENABLE_OSC)
            target_include_directories(kama-standalone PRIVATE ${LIBLO_INCLUDE_DIRS})
            target_link_libraries(kama-standalone ${LIBLO_LIBRARIES})
            message(STATUS "OSC support enabled")
        else()
            message(WARNING "OSC disabled - liblo not found")
        endif()
    endif()
    
    # Установка standalone
    install(TARGETS kama-standalone
        RUNTIME DESTINATION bin
    )
endif()

# LV2 плагин
if(KAMA_BUILD_TYPE STREQUAL "lv2" OR KAMA_BUILD_TYPE STREQUAL "both")
    message(STATUS "Building LV2 plugin")
    
    # Проверяем зависимости LV2
    find_package(PkgConfig REQUIRED)
    pkg_check_modules(LV2 REQUIRED lv2)
    pkg_check_modules(LILV REQUIRED lilv-0)
    
    if(LV2_FOUND AND LILV_FOUND)
        # LV2 плагин как shared library
        add_library(kama-lv2 MODULE
            src/lv2/plugin.cpp
            src/lv2/plugin_instance.cpp
        )
        
        target_compile_definitions(kama-lv2 PRIVATE
            KAMA_LV2_BUILD
            LV2_PLUGIN
        )
        
        target_include_directories(kama-lv2 PRIVATE
            include
            ${LV2_INCLUDE_DIRS}
            ${LILV_INCLUDE_DIRS}
        )
        
        target_link_libraries(kama-lv2
            kama_core
            ${LV2_LIBRARIES}
            ${LILV_LIBRARIES}
        )
        
        set_target_properties(kama-lv2 PROPERTIES
            PREFIX ""
            OUTPUT_NAME "kama"
        )
        
        # Установка LV2 плагина
        install(TARGETS kama-lv2
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/lv2/kama.lv2
        )
        
        # Установка TTL файлов
        install(FILES
            lv2/manifest.ttl
            lv2/kama.ttl
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/lv2/kama.lv2
        )
        
    else()
        if(KAMA_BUILD_TYPE STREQUAL "lv2")
            message(FATAL_ERROR "LV2 build requested but LV2/Lilv not found")
        else()
            message(WARNING "LV2 dependencies not found - skipping LV2 plugin build")
        endif()
    endif()
endif()

# Тестирование
if(ENABLE_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# Дополнительная информация о сборке
message(STATUS "=========================================================================")
message(STATUS "${PROJECT_FULL_NAME} configured successfully")
message(STATUS "Version: ${PROJECT_VERSION}")
message(STATUS "License: ${PROJECT_LICENSE}")
message(STATUS "Homepage: ${PROJECT_HOMEPAGE_URL}")
message(STATUS "=========================================================================")
