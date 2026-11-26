cmake_minimum_required(VERSION 3.18.1)
project(nekoflash_usbfs LANGUAGES CXX)

add_library(nekoflash_usbfs SHARED native_usbfs.cpp)

target_compile_features(nekoflash_usbfs PRIVATE cxx_std_17)
target_compile_options(nekoflash_usbfs PRIVATE -Wall -Wextra -Werror)

target_link_libraries(nekoflash_usbfs android log)

# Cargo creates a static archive per ABI and CMake build directory. Link JNI with
# the same NDK toolchain and minSdk as the rest of the Android application.
set(rust_abi_arm64-v8a aarch64-linux-android)
set(rust_abi_armeabi-v7a armv7-linux-androideabi)
set(rust_abi_x86 i686-linux-android)
set(rust_abi_x86_64 x86_64-linux-android)
set(rust_target "${rust_abi_${ANDROID_ABI}}")
if(NOT rust_target)
    message(FATAL_ERROR "Unsupported Rust Android ABI: ${ANDROID_ABI}")
endif()
find_program(CARGO cargo REQUIRED)
get_filename_component(rust_workspace "${CMAKE_CURRENT_SOURCE_DIR}/../../../../rust" ABSOLUTE)
set(cargo_output "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(rust_archive "${cargo_output}/${rust_target}/release/libnekoflash_android.a")
file(GLOB_RECURSE rust_inputs CONFIGURE_DEPENDS "${rust_workspace}/crates/*.rs" "${rust_workspace}/crates/Cargo.toml" "${rust_workspace}/crates/*/Cargo.toml")
add_custom_command(OUTPUT "${rust_archive}"
    COMMAND "${CARGO}" build --release --locked --target "${rust_target}"
            --target-dir "${cargo_output}" --package nekoflash-android
    WORKING_DIRECTORY "${rust_workspace}"
    DEPENDS ${rust_inputs} "${rust_workspace}/Cargo.toml" "${rust_workspace}/Cargo.lock"
    VERBATIM)
add_custom_target(nekoflash_rust_archive DEPENDS "${rust_archive}")
add_library(nekoflash_core SHARED rust_bridge.cpp rust_wireless.cpp)
add_dependencies(nekoflash_core nekoflash_rust_archive)
target_link_options(nekoflash_core PRIVATE "-Wl,--exclude-libs,ALL" "-Wl,-z,max-page-size=16384")
target_link_libraries(nekoflash_core PRIVATE "${rust_archive}" log dl m)
