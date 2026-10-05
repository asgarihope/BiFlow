#pragma once

// Identifiers duplicated between the C++ provider and Package.appxmanifest.
// Keep them here so "did you update both places?" is answerable by reading
// this file plus the manifest.

#include <winrt/base.h>

namespace BiFlow::Widget
{
    // CLSID of the COM class implementing IWidgetProvider. Must equal the
    // com:Class/@Id in the package manifest.
    inline constexpr winrt::guid ProviderClsid{
        0x7B3C1A62, 0x4F5D, 0x4E2B, {0x9C, 0x8A, 0x1D, 0x6E, 0x5A, 0x9F, 0x3B, 0x47}};

    // Widget definition id. Must equal the uap3:Definition/@Id in the
    // manifest; the host uses it to address a pinned widget instance.
    inline constexpr wchar_t const* WidgetDefinitionId = L"BiFlow_Traffic";
}
