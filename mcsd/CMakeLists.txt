cmake_minimum_required(VERSION 3.8)

project(mcsd LANGUAGES CXX VERSION 0.1.0)

add_subdirectory(lib/sqlite3)

file(GLOB SRC_FILES ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)

add_executable(mcsd ${SRC_FILES})
target_include_directories(mcsd PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_link_libraries(mcsd PRIVATE sqlite3)
