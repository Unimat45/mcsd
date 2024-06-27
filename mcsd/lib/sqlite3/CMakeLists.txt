cmake_minimum_required(VERSION 3.8.0)

project(sqlite VERSION 3)

option(BUILD_SHARED_LIBS "Build this library as a shared object" ON)

file(GLOB SRC_FILES ${CMAKE_CURRENT_SOURCE_DIR}/*.c)
file(GLOB HEADER_FILES ${CMAKE_CURRENT_SOURCE_DIR}/*.h)

if (BUILD_SHARED_LIBS)
    add_library(sqlite3 SHARED ${SRC_FILES} ${HEADER_FILES})
else()
    add_library(sqlite3 STATIC ${SRC_FILES} ${HEADER_FILES})
endif()

