//! Classes `Instance.new` refuses, mirroring the `NotCreatable` and `Service`
//! tags of `assets/API-Dump.json`.
//!
//! The list is embedded rather than read from the dump because
//! `rbx_reflection::ClassDescriptor` keeps only names, superclasses and
//! properties, not tags. Regenerate after a dump update with:
//! `python3 -c "import json;d=json.load(open('assets/API-Dump.json'));print(sorted({c['Name'] for c in d['Classes'] if set(c.get('Tags') or []) & {'NotCreatable','Service'}}))"`

// Sorted, so the lookup below can binary search it. Formatting is frozen because
// rustfmt would give each of the 394 names a line of its own, well past the
// project's file-length limit.
#[rustfmt::skip]
const BLOCKED: &[&str] = &[
    "AdService", "AnalysticsSettings", "AnalyticsService", "AnimationClip",
    "AnimationClipProvider", "AnimationFromVideoCreatorService",
    "AnimationFromVideoCreatorStudioService", "AnimationImportData", "AnimationStreamTrack",
    "AnimationTrack", "AppStorageService", "AppUpdateService", "AssetCounterService",
    "AssetDeliveryProxy", "AssetImportService", "AssetImportSession", "AssetManagerService",
    "AssetPatchSettings", "AssetService", "AssetSoundEffect", "AudioPages", "AvatarChatService",
    "AvatarEditorService", "AvatarImportService", "BackpackItem", "BadgeService",
    "BaseImportData", "BasePart", "BasePlayerGui", "BaseScript", "BaseWrap", "BevelMesh",
    "BodyMover", "BrowserService", "BubbleChatConfiguration", "BulkImportService",
    "CSGDictionaryService", "CacheableContentProvider", "CalloutService", "CaptureService",
    "CatalogPages", "ChangeHistoryService", "ChannelSelectorSoundEffect", "CharacterAppearance",
    "Chat", "ChatInputBarConfiguration", "ChatWindowConfiguration", "ClientReplicator",
    "Clothing", "CloudLocalizationTable", "ClusterPacketCache", "CollectionService",
    "CommandInstance", "CommandService", "ConfigureServerService", "Constraint",
    "ContentProvider", "ContextActionService", "Controller", "ControllerBase",
    "ControllerSensor", "ControllerService", "CookiesService", "CoreGui", "CorePackages",
    "CoreScript", "CoreScriptDebuggingManagerHelper", "CoreScriptSyncService",
    "CrossDMScriptChangeListener", "CustomSoundEffect", "DataModel", "DataModelMesh",
    "DataModelPatchService", "DataModelSession", "DataStore", "DataStoreInfo", "DataStoreKey",
    "DataStoreKeyInfo", "DataStoreKeyPages", "DataStoreListingPages",
    "DataStoreObjectVersionInfo", "DataStorePages", "DataStoreService", "DataStoreVersionPages",
    "Debris", "DebugSettings", "DebuggablePluginWatcher", "DebuggerBreakpoint",
    "DebuggerConnection", "DebuggerConnectionManager", "DebuggerLuaResponse", "DebuggerManager",
    "DebuggerUIService", "DebuggerVariable", "DeviceIdService", "DockWidgetPluginGui",
    "DraftsService", "DraggerService", "DynamicImage", "DynamicRotate", "EmotesPages",
    "EventIngestService", "ExperienceAuthService", "FaceAnimatorService", "FaceInstance",
    "FacialAnimationRecordingService", "FacialAnimationStreamingServiceStats",
    "FacialAnimationStreamingServiceV2", "FacialAnimationStreamingSubsessionStats",
    "FacsImportData", "Feature", "File", "FlagStandService", "FlyweightService",
    "FormFactorPart", "FriendPages", "FriendService", "GamePassService", "GameSettings",
    "GamepadService", "GenericSettings", "Geometry", "GeometryService", "GlobalDataStore",
    "GlobalSettings", "GoogleAnalyticsConfiguration", "GroupImportData", "GroupService",
    "GuiBase", "GuiBase2d", "GuiBase3d", "GuiButton", "GuiLabel", "GuiObject", "GuiService",
    "GuidRegistryService", "HSRDataContentProvider", "HandleAdornment", "HandlesBase",
    "HapticService", "HeightmapImporterService", "Hopper", "HttpRbxApiService", "HttpRequest",
    "HttpService", "ILegacyStudioBridge", "IXPService", "IncrementalPatchBuilder",
    "InputObject", "InsertService", "Instance", "InstanceAdornment", "InventoryPages",
    "JointImportData", "JointInstance", "JointsService", "KeyboardService",
    "KeyframeSequenceProvider", "LSPFileSyncService", "LanguageService", "LayerCollector",
    "LegacyStudioBridge", "Light", "Lighting", "LiveScriptingService",
    "LocalDebuggerConnection", "LocalStorageService", "LocalizationService", "LodDataEntity",
    "LodDataService", "LogService", "LoginService", "LuaSettings", "LuaSourceContainer",
    "LuaWebService", "LuauScriptAnalyzerService", "ManualSurfaceJointInstance",
    "MarketplaceService", "MaterialGenerationService", "MaterialGenerationSession",
    "MaterialImportData", "MaterialService", "MemStorageConnection", "MemStorageService",
    "MemoryStoreQueue", "MemoryStoreService", "MemoryStoreSortedMap", "MeshContentProvider",
    "MeshImportData", "MessageBusConnection", "MessageBusService", "MessagingService",
    "MetaBreakpoint", "MetaBreakpointContext", "MetaBreakpointManager", "Mouse", "MouseService",
    "MultipleDocumentInterfaceInstance", "NetworkClient", "NetworkMarker", "NetworkPeer",
    "NetworkReplicator", "NetworkServer", "NetworkSettings",
    "NonReplicatedCSGDictionaryService", "NotificationService", "OmniRecommendationsService",
    "OpenCloudApiV1", "OpenCloudService", "OrderedDataStore", "OutfitPages", "PVAdornment",
    "PVInstance", "PackageLink", "PackageService", "PackageUIService", "Pages", "PartAdornment",
    "PatchBundlerFileWatch", "PatchMapping", "Path", "PathfindingService", "PausedState",
    "PausedStateBreakpoint", "PausedStateException", "PermissionsService", "PhysicsService",
    "PhysicsSettings", "PlaceStatsService", "PlacesService", "Platform",
    "PlayerEmulatorService", "PlayerGui", "PlayerMouse", "PlayerScripts", "Players", "Plugin",
    "PluginDebugService", "PluginDragEvent", "PluginGui", "PluginGuiService",
    "PluginManagementService", "PluginManager", "PluginManagerInterface", "PluginMenu",
    "PluginMouse", "PluginPolicyService", "PluginToolbar", "PluginToolbarButton",
    "PointsService", "PolicyService", "PoseBase", "PostEffect", "ProcessInstancePhysicsService",
    "ProximityPromptService", "PublishService", "QWidgetPluginGui", "RbxAnalyticsService",
    "ReflectionMetadataItem", "RemoteCursorService", "RemoteDebuggerServer", "RenderSettings",
    "ReplicatedFirst", "ReplicatedStorage", "RobloxPluginGuiService", "RobloxReplicatedStorage",
    "RobloxServerStorage", "RomarkService", "RootImportData", "RtMessagingService",
    "RunService", "RunningAverageItemDouble", "RunningAverageItemInt",
    "RunningAverageTimeIntervalItem", "RuntimeScriptService", "SafetyService", "ScreenshotHud",
    "ScriptBuilder", "ScriptChangeService", "ScriptCloneWatcher", "ScriptCloneWatcherHelper",
    "ScriptCommitService", "ScriptContext", "ScriptDebugger", "ScriptDocument",
    "ScriptEditorService", "ScriptRegistrationService", "ScriptRuntime", "ScriptService",
    "Selection", "SelectionHighlightManager", "SelectionLasso", "SensorBase",
    "ServerReplicator", "ServerScriptService", "ServerStorage", "ServiceProvider",
    "ServiceVisibilityService", "SessionService", "SharedTableRegistry",
    "ShorelineUpgraderService", "SlidingBallConstraint", "SmoothVoxelsUpgraderService",
    "SnippetService", "SocialService", "SolidModelContentProvider", "SoundEffect",
    "SoundService", "SpawnerService", "StackFrame", "StandardPages", "StarterCharacterScripts",
    "StarterGui", "StarterPack", "StarterPlayer", "StarterPlayerScripts", "Stats", "StatsItem",
    "Status", "StopWatchReporter", "Studio", "StudioAssetService", "StudioData",
    "StudioDeviceEmulatorService", "StudioPublishService", "StudioScriptDebugEventListener",
    "StudioSdkService", "StudioService", "StudioTheme", "StyleBase", "StylingService",
    "SurfaceGuiBase", "SyncScriptBuilder", "TaskScheduler", "TeamCreateData",
    "TeamCreatePublishService", "TeamCreateService", "Teams", "TeleportAsyncResult",
    "TeleportService", "TemporaryCageMeshProvider", "TemporaryScriptService", "Terrain",
    "TestService", "TextBoxService", "TextChatConfigurations", "TextChatMessage",
    "TextChatService", "TextFilterResult", "TextFilterTranslatedResult", "TextService",
    "TextSource", "ThirdPartyUserService", "ThreadState", "TimerService",
    "ToastNotificationService", "TotalCountTimeIntervalItem", "TouchInputService",
    "TouchTransmitter", "TracerService", "TrackerLodController", "Translator",
    "TriangleMeshPart", "TutorialService", "TweenBase", "TweenService", "UGCAvatarService",
    "UGCValidationService", "UIBase", "UIComponent", "UIConstraint", "UIGridStyleLayout",
    "UILayout", "UnvalidatedAssetService", "UserGameSettings", "UserInputService",
    "UserService", "UserSettings", "UserStorageService", "VRService", "VRStatusService",
    "ValueBase", "VersionControlService", "VideoCaptureService", "VideoService",
    "VirtualInputManager", "VirtualUser", "VisibilityCheckDispatcher", "VisibilityService",
    "Visit", "VoiceChatInternal", "VoiceChatService", "Workspace", "WorldRoot"
];

pub(crate) fn is_blocked(class: &str) -> bool {
    BLOCKED.binary_search(&class).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_and_abstract_bases_are_blocked() {
        assert!(is_blocked("Workspace"));
        assert!(is_blocked("BasePart"));
        assert!(is_blocked("Players"));
    }

    #[test]
    fn ordinary_classes_are_allowed() {
        assert!(!is_blocked("Part"));
        assert!(!is_blocked("Folder"));
        assert!(!is_blocked("SpawnLocation"));
    }

    #[test]
    fn list_stays_sorted() {
        assert!(BLOCKED.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
