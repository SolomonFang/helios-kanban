import { useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { RJSFSchema } from '@rjsf/utils';
import { BaseCodingAgent } from 'shared/types';
import type { AcpConfigOption } from 'shared/types';
import { opencodeApi } from '@/lib/api';

type ExecutorFormData = Record<string, unknown>;

function enumSchema(
  options: AcpConfigOption[],
  currentValue: unknown
): Pick<RJSFSchema, 'enum' | 'enumNames'> | undefined {
  if (options.length === 0) return undefined;

  const values = options.map((option) => option.value);
  if (typeof currentValue === 'string' && !values.includes(currentValue)) {
    options = [
      ...options,
      {
        value: currentValue,
        label: currentValue,
      },
    ];
  }

  return {
    enum: [null, ...options.map((option) => option.value)],
    enumNames: ['Not specified', ...options.map((option) => option.label)],
  };
}

export function useOpencodeConfigSchema(
  executor: BaseCodingAgent,
  baseSchema: RJSFSchema,
  formData: unknown
) {
  const { data } = useQuery({
    queryKey: ['opencode', 'config-options'],
    queryFn: opencodeApi.configOptions,
    enabled: executor === BaseCodingAgent.OPENCODE,
    staleTime: 5 * 60 * 1000,
    retry: false,
  });

  const values = (formData || {}) as ExecutorFormData;

  const schema = useMemo(() => {
    if (!data || executor !== BaseCodingAgent.OPENCODE) return baseSchema;

    const currentModel =
      typeof values.model === 'string' ? values.model : data.current_model;
    const model = data.models.find((option) => option.value === currentModel);
    const modelSchema = enumSchema(data.models, values.model);
    const variantSchema = model
      ? enumSchema(model.variants, values.variant)
      : undefined;
    const agentSchema = enumSchema(data.agents, values.agent);

    if (!modelSchema && !variantSchema && !agentSchema) return baseSchema;

    return {
      ...baseSchema,
      properties: {
        ...baseSchema.properties,
        ...(modelSchema
          ? {
              model: {
                ...(typeof baseSchema.properties?.model === 'object'
                  ? baseSchema.properties.model
                  : {}),
                ...modelSchema,
              },
            }
          : {}),
        ...(variantSchema
          ? {
              variant: {
                ...(typeof baseSchema.properties?.variant === 'object'
                  ? baseSchema.properties.variant
                  : {}),
                ...variantSchema,
              },
            }
          : {}),
        ...(agentSchema
          ? {
              agent: {
                ...(typeof baseSchema.properties?.agent === 'object'
                  ? baseSchema.properties.agent
                  : {}),
                ...agentSchema,
              },
            }
          : {}),
      },
    };
  }, [baseSchema, data, executor, values.agent, values.model, values.variant]);

  const normalizeFormData = useCallback(
    (previous: unknown, next: unknown): unknown => {
      if (!data || executor !== BaseCodingAgent.OPENCODE) return next;

      const previousValues = (previous || {}) as ExecutorFormData;
      const nextValues = { ...((next || {}) as ExecutorFormData) };
      if (previousValues.model === nextValues.model) return next;

      const model = data.models.find(
        (option) => option.value === nextValues.model
      );
      if (
        nextValues.variant &&
        !model?.variants.some((option) => option.value === nextValues.variant)
      ) {
        delete nextValues.variant;
        return nextValues;
      }
      return next;
    },
    [data, executor]
  );

  return { schema, normalizeFormData };
}
