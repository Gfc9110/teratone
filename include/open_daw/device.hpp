#pragma once
#include "open_daw/service.hpp"
#include <memory>
namespace daw {
enum class DeviceMode { Native, SilentClock };
// All lifecycle calls on the control thread. stop() is a join/quiescence barrier.
class OutputDevice {
public:
    OutputDevice();
    ~OutputDevice();
    OutputDevice(const OutputDevice&) = delete;
    OutputDevice& operator=(const OutputDevice&) = delete;
    void start(EngineService& service, DeviceMode mode);
    void stop() noexcept;
    bool open() const noexcept;
    uint32_t errorCode() const noexcept;
private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};
}
