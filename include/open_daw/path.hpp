#pragma once
#include <filesystem>
#include <string_view>
namespace daw {
inline std::filesystem::path utf8Path(std::string_view value) {
    return std::filesystem::path(std::u8string(value.begin(), value.end()));
}
}
