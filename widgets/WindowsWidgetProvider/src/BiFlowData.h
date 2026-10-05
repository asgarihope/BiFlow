#pragma once

// Everything one refresh of the card needs, plus the two readers that
// produce it. No state, no privilege, no dependency on the BiFlow desktop.

#include <cstdint>
#include <string>

namespace BiFlow::Widget
{
    struct Snapshot
    {
        bool controllerReachable{false};
        bool connected{false};
        std::uint64_t uploadedTotal{0};
        std::uint64_t downloadedTotal{0};
        std::wstring defaultRouteLabel{L"\xE2\x80\x94"};  // em dash
        int enabledClients{0};
        std::wstring failureReason;

        static Snapshot unreachable(std::wstring reason)
        {
            Snapshot s;
            s.failureReason = std::move(reason);
            return s;
        }
    };

    // Reads %APPDATA%\biflow\config.toml and the local Mihomo REST
    // controller. Never throws; every failure becomes a reason string.
    Snapshot readSnapshot();

    // Adaptive Card JSON, the only template format the widget host accepts
    // for a native widget. `withTotals` is the medium card; the small card
    // drops the routing caption so the totals stay legible.
    std::wstring buildCard(Snapshot const& snapshot, bool withTotals);

    // Binary units, matching the desktop status bar exactly.
    std::wstring formatBytes(std::uint64_t bytes);
}
