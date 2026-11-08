cmake_minimum_required(VERSION 3.22)
project(fah_android LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(ncnn_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../../../third_party/ncnn-20261020-android-vulkan/${ANDROID_ABI}/lib/cmake/ncnn")
find_package(ncnn REQUIRED)
get_target_property(ncnn_options ncnn INTERFACE_COMPILE_OPTIONS)
list(REMOVE_ITEM ncnn_options "-fno-exceptions")
set_target_properties(ncnn PROPERTIES INTERFACE_COMPILE_OPTIONS "${ncnn_options}")
add_compile_options(-fexceptions -O3)

if(NOT ANDROID_ABI STREQUAL "arm64-v8a")
    message(FATAL_ERROR "The Rust build currently supports arm64-v8a only")
endif()
find_program(CARGO_EXECUTABLE cargo REQUIRED)
set(RUST_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../rust")
set(RUST_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(RUST_LIBRARY "${RUST_TARGET_DIR}/aarch64-linux-android/release/libfah_rust.a")
file(GLOB_RECURSE RUST_SOURCES CONFIGURE_DEPENDS "${RUST_DIR}/src/*.rs")
add_custom_command(OUTPUT "${RUST_LIBRARY}"
    COMMAND "${CMAKE_COMMAND}" -E env "CARGO_TARGET_DIR=${RUST_TARGET_DIR}"
        "${CARGO_EXECUTABLE}" build --manifest-path "${RUST_DIR}/Cargo.toml"
        --target aarch64-linux-android --release --locked
    DEPENDS ${RUST_SOURCES} "${RUST_DIR}/Cargo.toml" "${RUST_DIR}/Cargo.lock"
    VERBATIM)
add_custom_target(fah-rust-build DEPENDS "${RUST_LIBRARY}")
add_library(fah-rust STATIC IMPORTED)
set_target_properties(fah-rust PROPERTIES IMPORTED_LOCATION "${RUST_LIBRARY}")
add_dependencies(fah-rust fah-rust-build)
add_library(fah-backend STATIC bridge.cpp runtime.cpp vulkan_ops.cpp)
target_link_libraries(fah-backend PUBLIC ncnn)
# Whole-archive retains the JNI entry points exported by the Rust static library.
add_library(fah SHARED anchor.cpp)
target_link_libraries(fah PRIVATE "-Wl,--whole-archive" fah-rust "-Wl,--no-whole-archive" fah-backend android log jnigraphics dl m)
add_executable(fah-check check.cpp)
target_link_libraries(fah-check PRIVATE fah-rust fah-backend android log jnigraphics dl m)
