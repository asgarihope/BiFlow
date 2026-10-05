namespace BiFlow.WidgetProvider;

/// <summary>
/// The identifiers that are duplicated between <c>Program.cs</c> and
/// <c>Package.appxmanifest</c>. Keeping them in one file makes the
/// "did you update all three places?" question answerable by reading
/// this file plus the manifest.
/// </summary>
internal static class WidgetIds
{
    /// <summary>CLSID of the COM class implementing <c>IWidgetProvider</c>.</summary>
    public const string ProviderClsid = "7B3C1A62-4F5D-4E2B-9C8A-1D6E5A9F3B47";

    /// <summary>
    /// Widget definition id. Must equal the <c>Id</c> attribute of the
    /// <c>Definition</c> element in the package manifest; the host uses
    /// it to address a pinned widget instance.
    /// </summary>
    public const string WidgetDefinitionId = "BiFlow_Traffic";

    /// <summary>Widget id used for the single pinned instance we support.</summary>
    public const string DefaultWidgetId = "BiFlow_Traffic_Default";
}
