#ifndef CONFIG_H
#define CONFIG_H

#include <vector>
#include <memory>
#include <string>

class Config {
private:
    int id;
    std::string title;
    std::string directory;
    int max_ram;
    int min_ram;
    std::string version;

public:
    Config(int id, std::string title, std::string directory, std::string version, int min, int max): id(id), title(title), directory(directory), version(version), max_ram(max), min_ram(min) {}

    int get_id();
    std::string get_title();
    std::string get_directory();
    std::string get_version();
    int get_max_ram();
    int get_min_ram();

    static std::vector<std::shared_ptr<Config>> query(const char *cmd, ...);
};

#endif // CONFIG_H
