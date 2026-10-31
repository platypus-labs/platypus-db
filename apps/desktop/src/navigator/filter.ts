/** The navigator's text filter, which reads `schema.object` as well as a bare name. */

export interface TextFilter {
  /** Whether the filter names the schema as a whole: all of its objects show. */
  wholeSchema: (schema: string) => boolean;
  /** Whether an object of `schema` shows. */
  matches: (schema: string, object: string) => boolean;
}

/** `needle` is lower case. A bare text matches the names of objects and of schemas (a
 *  schema that matches shows everything in it); `schema.object` the objects of the schemas
 *  that contain `schema`, and `schema.` all of them. */
export function textFilter(needle: string): TextFilter {
  const dot = needle.indexOf(".");
  const schemaPart = dot < 0 ? needle : needle.slice(0, dot);
  const objectPart = dot < 0 ? "" : needle.slice(dot + 1);
  const inSchema = (schema: string) => schema.toLowerCase().includes(schemaPart);
  return {
    wholeSchema: (schema) => needle !== "" && objectPart === "" && inSchema(schema),
    matches: (schema, object) => {
      if (!needle) return true;
      const name = object.toLowerCase();
      return name.includes(needle) || (inSchema(schema) && name.includes(objectPart));
    },
  };
}
