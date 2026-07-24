import React from "react";
import { useTranslation } from "react-i18next";
import type { ModelInfo } from "@/bindings";
import {
  getTranslatedModelName,
  getTranslatedModelDescription,
} from "../../lib/utils/modelTranslation";
import {
  OPENAI_TRANSCRIPTION_ROUTE_ID,
  type OpenAiTranscriptionAvailability,
} from "@/lib/transcriptionRoute";

interface ModelDropdownProps {
  models: ModelInfo[];
  currentModelId: string;
  activeRoute: "local" | "openai";
  openAiOption: {
    label: string;
    availability: OpenAiTranscriptionAvailability;
  } | null;
  onModelSelect: (modelId: string) => void;
  onOpenAiSelect: () => void;
}

const ModelDropdown: React.FC<ModelDropdownProps> = ({
  models,
  currentModelId,
  activeRoute,
  openAiOption,
  onModelSelect,
  onOpenAiSelect,
}) => {
  const { t } = useTranslation();
  const downloadedModels = models.filter((m) => m.is_downloaded);
  const hasOpenAiOption = openAiOption?.availability.enabled ?? false;
  const hasAnyOption = hasOpenAiOption || downloadedModels.length > 0;

  const handleModelClick = (modelId: string) => {
    onModelSelect(modelId);
  };

  const handleOpenAiClick = () => {
    onOpenAiSelect();
  };

  return (
    <div className="absolute bottom-full start-0 mb-2 w-64 max-h-[60vh] overflow-y-auto bg-background border border-mid-gray/20 rounded-lg shadow-lg py-2 z-50">
      {hasAnyOption ? (
        <div>
          {hasOpenAiOption && openAiOption && (
            <div
              key={OPENAI_TRANSCRIPTION_ROUTE_ID}
              onClick={handleOpenAiClick}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  handleOpenAiClick();
                }
              }}
              tabIndex={0}
              role="button"
              aria-disabled={!openAiOption.availability.configured}
              className={`w-full px-3 py-2 text-start transition-colors focus:outline-none ${
                openAiOption.availability.configured
                  ? "hover:bg-mid-gray/10 cursor-pointer"
                  : "cursor-not-allowed opacity-60"
              } ${
                activeRoute === "openai"
                  ? "bg-logo-primary/10 text-logo-primary"
                  : ""
              }`}
            >
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-sm text-text/80">
                    {openAiOption.label}
                  </div>
                  <div className="text-xs text-text/40 italic pe-4">
                    {openAiOption.availability.configured
                      ? t("modelSelector.openaiCloudDescription")
                      : t(openAiOption.availability.missingReasonKey ?? "")}
                  </div>
                </div>
                {activeRoute === "openai" && (
                  <div className="text-xs text-logo-primary">
                    {t("modelSelector.active")}
                  </div>
                )}
              </div>
            </div>
          )}
          {downloadedModels.map((model) => (
            <div
              key={model.id}
              onClick={() => handleModelClick(model.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  handleModelClick(model.id);
                }
              }}
              tabIndex={0}
              role="button"
              className={`w-full px-3 py-2 text-start hover:bg-mid-gray/10 transition-colors cursor-pointer focus:outline-none ${
                activeRoute === "local" && currentModelId === model.id
                  ? "bg-logo-primary/10 text-logo-primary"
                  : ""
              }`}
            >
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-sm text-text/80">
                    {getTranslatedModelName(model, t)}
                    {model.is_custom && (
                      <span className="ms-1.5 text-[10px] font-medium text-text/40 uppercase">
                        {t("modelSelector.custom")}
                      </span>
                    )}
                    {model.supports_streaming && (
                      <span className="ms-1.5 text-[10px] font-medium text-logo-primary/70 uppercase">
                        {t("modelSelector.streaming")}
                      </span>
                    )}
                  </div>
                  <div className="text-xs text-text/40 italic pe-4">
                    {getTranslatedModelDescription(model, t)}
                  </div>
                </div>
                {activeRoute === "local" && currentModelId === model.id && (
                  <div className="text-xs text-logo-primary">
                    {t("modelSelector.active")}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="px-3 py-2 text-sm text-text/60">
          {t("modelSelector.noModelsAvailable")}
        </div>
      )}
    </div>
  );
};

export default ModelDropdown;
