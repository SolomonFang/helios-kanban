import { ObjectFieldTemplateProps } from '@rjsf/utils';

export const ObjectFieldTemplate = (props: ObjectFieldTemplateProps) => {
  const { title, description, properties, fieldPathId } = props;
  const isRoot = fieldPathId.$id === 'root';

  if (isRoot) {
    return (
      <div className="divide-y">
        {properties.map((element) => (
          <div key={element.name}>{element.content}</div>
        ))}
      </div>
    );
  }

  // Nested objects get a section treatment so long forms scan better
  return (
    <fieldset className="space-y-3 py-4">
      {(title || description) && (
        <div className="space-y-1">
          {title && (
            <legend className="text-sm font-bold leading-relaxed">
              {title}
            </legend>
          )}
          {description && (
            <p className="text-sm text-muted-foreground leading-relaxed">
              {description}
            </p>
          )}
        </div>
      )}
      <div className="divide-y rounded-md border px-4">
        {properties.map((element) => (
          <div key={element.name}>{element.content}</div>
        ))}
      </div>
    </fieldset>
  );
};
