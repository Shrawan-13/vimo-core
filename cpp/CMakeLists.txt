cmake_minimum_required(VERSION 3.22.1)
project(vimo_oboe_shim CXX)

# The Oboe shim.
#
# Oboe is C++, so the Rust platform layer reaches it through a small C
# interface. This builds the shim and the Oboe library it needs, and puts
# both where the Rust linker can find them.
#
# The release workflow calls this once per ABI, with the Android NDK's
# toolchain file and the output directory of that ABI.

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

include(FetchContent)

# Oboe itself. The sample apps and the tests are not needed.
set(OBOE_BUILD_TESTING OFF CACHE BOOL "" FORCE)
set(OBOE_BUILD_SAMPLE OFF CACHE BOOL "" FORCE)
FetchContent_Declare(
  oboe
  GIT_REPOSITORY https://github.com/google/oboe.git
  GIT_TAG 1.8.0
  GIT_SHALLOW TRUE
)
FetchContent_MakeAvailable(oboe)

# The shim.
add_library(vimo_oboe SHARED vimo_oboe.cpp)
target_link_libraries(vimo_oboe oboe log)
target_compile_options(vimo_oboe PRIVATE -O2 -fvisibility=hidden)

# Both libraries land in the directory the caller asked for, which is also
# where the Rust linker looks for them.
if(CMAKE_LIBRARY_OUTPUT_DIRECTORY)
  set_target_properties(vimo_oboe PROPERTIES
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_LIBRARY_OUTPUT_DIRECTORY}")
endif()
