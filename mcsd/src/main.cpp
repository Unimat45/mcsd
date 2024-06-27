#include <iostream>
#include "Config.hpp"

int main(void) {
    std::vector<std::shared_ptr<Config>> servers = Config::query("SELECT * FROM Servers");

    for (auto& p : servers) {
        std::cout << p->get_title() << "\n";
    }

    return 0;
}
