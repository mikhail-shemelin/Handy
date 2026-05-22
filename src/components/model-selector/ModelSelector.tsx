import React, { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type TranscriptionProvider } from "@/bindings";
import { getTranslatedModelName } from "../../lib/utils/modelTranslation";
import { useModelStore } from "../../stores/modelStore";
import { useSettings } from "@/hooks/useSettings";
import {
  getOpenAiTranscriptionAvailability,
  getOpenAiTranscriptionModel,
  isOpenAiTranscriptionActive,
} from "@/lib/transcriptionRoute";
import ModelStatusButton from "./ModelStatusButton";
import ModelDropdown from "./ModelDropdown";
import DownloadProgressDisplay from "./DownloadProgressDisplay";

import { ModelStateEvent } from "@/lib/types/events";

type ModelStatus =
  | "ready"
  | "loading"
  | "downloading"
  | "verifying"
  | "extracting"
  | "error"
  | "unloaded"
  | "none";

interface ModelSelectorProps {
  onError?: (error: string) => void;
}

const ModelSelector: React.FC<ModelSelectorProps> = ({ onError }) => {
  const { t } = useTranslation();
  const {
    models,
    currentModel,
    downloadProgress,
    downloadStats,
    verifyingModels,
    extractingModels,
    selectModel,
  } = useModelStore();
  const { settings, refreshSettings } = useSettings();

  const [modelStatus, setModelStatus] = useState<ModelStatus>("unloaded");
  const [modelError, setModelError] = useState<string | null>(null);
  const [showModelDropdown, setShowModelDropdown] = useState(false);
  // Track pending model switch for optimistic display
  const [pendingModelId, setPendingModelId] = useState<string | null>(null);

  const dropdownRef = useRef<HTMLDivElement>(null);

  const displayModelId = pendingModelId || currentModel;
  const isOpenAiActive = isOpenAiTranscriptionActive(settings);
  const openAiAvailability = getOpenAiTranscriptionAvailability(settings);
  const openAiLabel = `${t("settings.transcription.provider.options.openai")} · ${getOpenAiTranscriptionModel(settings)}`;

  // Check model status when currentModel changes
  useEffect(() => {
    const checkStatus = async () => {
      if (isOpenAiActive) {
        setModelStatus(openAiAvailability.configured ? "ready" : "error");
        setModelError(null);
        return;
      }

      if (currentModel) {
        try {
          const statusResult = await commands.getTranscriptionModelStatus();
          if (statusResult.status === "ok") {
            setModelStatus(
              statusResult.data === currentModel ? "ready" : "unloaded",
            );
          }
        } catch {
          setModelStatus("error");
          setModelError("Failed to check model status");
        }
      } else {
        setModelStatus("none");
      }
    };
    checkStatus();
  }, [currentModel, isOpenAiActive, openAiAvailability.configured]);

  useEffect(() => {
    // Listen for model loading lifecycle events
    const modelStateUnlisten = listen<ModelStateEvent>(
      "model-state-changed",
      (event) => {
        const { event_type, error } = event.payload;
        switch (event_type) {
          case "loading_started":
            setModelStatus("loading");
            setModelError(null);
            break;
          case "loading_completed":
            setModelStatus("ready");
            setModelError(null);
            setPendingModelId(null);
            break;
          case "loading_failed":
            setModelStatus("error");
            setModelError(error || "Failed to load model");
            setPendingModelId(null);
            break;
          case "unloaded":
            setModelStatus("unloaded");
            setModelError(null);
            break;
        }
      },
    );

    // Auto-select model when download completes (fires after extraction too)
    const downloadCompleteUnlisten = listen<string>(
      "model-download-complete",
      (event) => {
        const modelId = event.payload;
        setTimeout(async () => {
          try {
            const isRecording = await commands.isRecording();
            if (!isRecording) {
              setPendingModelId(modelId);
              setModelError(null);
              setShowModelDropdown(false);
              const result =
                await commands.autoSelectLocalModelIfActiveRouteIsLocal(
                  modelId,
                );
              if (result.status === "error") {
                throw new Error(String(result.error));
              }

              if (!result.data) {
                setPendingModelId(null);
                return;
              }
              useModelStore.getState().setCurrentModel(modelId);
              await refreshSettings();
            }
          } catch {
            // Ignore errors in auto-select
          }
        }, 500);
      },
    );

    // Click outside to close dropdown
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setShowModelDropdown(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      modelStateUnlisten.then((fn) => fn());
      downloadCompleteUnlisten.then((fn) => fn());
    };
  }, [isOpenAiActive, refreshSettings, selectModel]);

  const handleModelSelect = async (modelId: string) => {
    setPendingModelId(modelId);
    setModelError(null);
    setShowModelDropdown(false);
    const success = await selectModel(modelId);
    if (!success) {
      setPendingModelId(null);
      setModelStatus("error");
      setModelError("Failed to switch model");
      onError?.("Failed to switch model");
      return;
    }

    if (isOpenAiActive) {
      const result = await commands.changeTranscriptionProviderSetting(
        "local" as TranscriptionProvider,
      );
      if (result.status === "error") {
        setModelStatus("error");
        setModelError(String(result.error));
        onError?.(String(result.error));
      }
      await refreshSettings();
    }
  };

  const handleOpenAiSelect = async () => {
    if (!openAiAvailability.configured) {
      const reason = openAiAvailability.missingReasonKey
        ? t(openAiAvailability.missingReasonKey)
        : t("modelSelector.modelError");
      setModelStatus("error");
      setModelError(reason);
      onError?.(reason);
      return;
    }

    setPendingModelId(null);
    setModelError(null);
    setShowModelDropdown(false);
    const result = await commands.changeTranscriptionProviderSetting(
      "openai" as TranscriptionProvider,
    );
    if (result.status === "error") {
      const error = String(result.error);
      setModelStatus("error");
      setModelError(error);
      onError?.(error);
    }
    await refreshSettings();
  };

  const getModelDisplayText = (): string => {
    if (isOpenAiActive) {
      if (!openAiAvailability.configured) {
        return t("modelSelector.openaiConfigurationRequired", {
          provider: t("settings.transcription.provider.options.openai"),
        });
      }

      return openAiLabel;
    }

    const verifyingKeys = Object.keys(verifyingModels);
    if (verifyingKeys.length > 0) {
      if (verifyingKeys.length === 1) {
        const modelId = verifyingKeys[0];
        const model = models.find((m) => m.id === modelId);
        const modelName = model
          ? getTranslatedModelName(model, t)
          : t("modelSelector.verifyingGeneric").replace("...", "");
        return t("modelSelector.verifying", { modelName });
      }

      return t("modelSelector.verifyingGeneric");
    }

    const extractingKeys = Object.keys(extractingModels);
    if (extractingKeys.length > 0) {
      if (extractingKeys.length === 1) {
        const modelId = extractingKeys[0];
        const model = models.find((m) => m.id === modelId);
        const modelName = model
          ? getTranslatedModelName(model, t)
          : t("modelSelector.extractingGeneric").replace("...", "");
        return t("modelSelector.extracting", { modelName });
      }

      return t("modelSelector.extractingMultiple", {
        count: extractingKeys.length,
      });
    }

    const progressValues = Object.values(downloadProgress);
    if (progressValues.length > 0) {
      if (progressValues.length === 1) {
        const progress = progressValues[0];
        const percentage = Math.max(
          0,
          Math.min(100, Math.round(progress.percentage)),
        );
        return t("modelSelector.downloading", { percentage });
      }

      return t("modelSelector.downloadingMultiple", {
        count: progressValues.length,
      });
    }

    const currentModelInfo = models.find((m) => m.id === displayModelId);

    switch (modelStatus) {
      case "ready":
        return currentModelInfo
          ? getTranslatedModelName(currentModelInfo, t)
          : t("modelSelector.modelReady");
      case "loading":
        return currentModelInfo
          ? t("modelSelector.loading", {
              modelName: getTranslatedModelName(currentModelInfo, t),
            })
          : t("modelSelector.loadingGeneric");
      case "extracting":
        return currentModelInfo
          ? t("modelSelector.extracting", {
              modelName: getTranslatedModelName(currentModelInfo, t),
            })
          : t("modelSelector.extractingGeneric");
      case "error":
        return modelError || t("modelSelector.modelError");
      case "unloaded":
        return currentModelInfo
          ? getTranslatedModelName(currentModelInfo, t)
          : t("modelSelector.modelUnloaded");
      case "none":
        return t("modelSelector.noModelDownloadRequired");
      default:
        return currentModelInfo
          ? getTranslatedModelName(currentModelInfo, t)
          : t("modelSelector.modelUnloaded");
    }
  };

  // Derive display status from model status + store state
  const getDisplayStatus = (): ModelStatus => {
    if (isOpenAiActive) {
      return openAiAvailability.configured ? "ready" : "error";
    }

    if (Object.keys(verifyingModels).length > 0) return "verifying";
    if (Object.keys(extractingModels).length > 0) return "extracting";
    if (Object.keys(downloadProgress).length > 0) return "downloading";
    return modelStatus;
  };

  return (
    <>
      {/* Model Status and Switcher */}
      <div className="relative" ref={dropdownRef}>
        <ModelStatusButton
          status={getDisplayStatus()}
          displayText={getModelDisplayText()}
          isDropdownOpen={showModelDropdown}
          onClick={() => setShowModelDropdown(!showModelDropdown)}
        />

        {/* Model Dropdown */}
        {showModelDropdown && (
          <ModelDropdown
            models={models}
            currentModelId={displayModelId}
            activeRoute={isOpenAiActive ? "openai" : "local"}
            openAiOption={
              openAiAvailability.enabled
                ? {
                    label: openAiLabel,
                    availability: openAiAvailability,
                  }
                : null
            }
            onModelSelect={handleModelSelect}
            onOpenAiSelect={handleOpenAiSelect}
          />
        )}
      </div>

      {/* Download Progress Bar for Models */}
      <DownloadProgressDisplay
        downloadProgress={downloadProgress}
        downloadStats={downloadStats}
      />
    </>
  );
};

export default ModelSelector;
