// scripts/export-lite.mjs
import { closeSync, fsyncSync, lstatSync as lstatSync2, openSync, readFileSync as readFileSync2, realpathSync as realpathSync2, renameSync, rmSync, writeSync } from "node:fs";
import { basename, dirname, isAbsolute as isAbsolute2, join, parse, relative as relative2, resolve as resolve2, sep as sep2 } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

// app/node_modules/js-yaml/dist/js-yaml.mjs
function getDefaultExportFromCjs(x) {
  return x && x.__esModule && Object.prototype.hasOwnProperty.call(x, "default") ? x["default"] : x;
}
var jsYaml = {};
var loader = {};
var common = {};
var hasRequiredCommon;
function requireCommon() {
  if (hasRequiredCommon) return common;
  hasRequiredCommon = 1;
  function isNothing(subject) {
    return typeof subject === "undefined" || subject === null;
  }
  function isObject(subject) {
    return typeof subject === "object" && subject !== null;
  }
  function toArray(sequence) {
    if (Array.isArray(sequence)) return sequence;
    else if (isNothing(sequence)) return [];
    return [sequence];
  }
  function extend(target, source) {
    if (source) {
      const sourceKeys = Object.keys(source);
      for (let index = 0, length = sourceKeys.length; index < length; index += 1) {
        const key = sourceKeys[index];
        target[key] = source[key];
      }
    }
    return target;
  }
  function repeat(string, count) {
    let result = "";
    for (let cycle = 0; cycle < count; cycle += 1) {
      result += string;
    }
    return result;
  }
  function isNegativeZero(number) {
    return number === 0 && Number.NEGATIVE_INFINITY === 1 / number;
  }
  common.isNothing = isNothing;
  common.isObject = isObject;
  common.toArray = toArray;
  common.repeat = repeat;
  common.isNegativeZero = isNegativeZero;
  common.extend = extend;
  return common;
}
var exception;
var hasRequiredException;
function requireException() {
  if (hasRequiredException) return exception;
  hasRequiredException = 1;
  function formatError(exception2, compact) {
    let where = "";
    const message = exception2.reason || "(unknown reason)";
    if (!exception2.mark) return message;
    if (exception2.mark.name) {
      where += 'in "' + exception2.mark.name + '" ';
    }
    where += "(" + (exception2.mark.line + 1) + ":" + (exception2.mark.column + 1) + ")";
    if (!compact && exception2.mark.snippet) {
      where += "\n\n" + exception2.mark.snippet;
    }
    return message + " " + where;
  }
  function YAMLException2(reason, mark) {
    Error.call(this);
    this.name = "YAMLException";
    this.reason = reason;
    this.mark = mark;
    this.message = formatError(this, false);
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, this.constructor);
    } else {
      this.stack = new Error().stack || "";
    }
  }
  YAMLException2.prototype = Object.create(Error.prototype);
  YAMLException2.prototype.constructor = YAMLException2;
  YAMLException2.prototype.toString = function toString(compact) {
    return this.name + ": " + formatError(this, compact);
  };
  exception = YAMLException2;
  return exception;
}
var snippet;
var hasRequiredSnippet;
function requireSnippet() {
  if (hasRequiredSnippet) return snippet;
  hasRequiredSnippet = 1;
  const common2 = requireCommon();
  function getLine(buffer, lineStart, lineEnd, position, maxLineLength) {
    let head = "";
    let tail = "";
    const maxHalfLength = Math.floor(maxLineLength / 2) - 1;
    if (position - lineStart > maxHalfLength) {
      head = " ... ";
      lineStart = position - maxHalfLength + head.length;
    }
    if (lineEnd - position > maxHalfLength) {
      tail = " ...";
      lineEnd = position + maxHalfLength - tail.length;
    }
    return {
      str: head + buffer.slice(lineStart, lineEnd).replace(/\t/g, "\u2192") + tail,
      pos: position - lineStart + head.length
      // relative position
    };
  }
  function padStart(string, max) {
    return common2.repeat(" ", max - string.length) + string;
  }
  function makeSnippet(mark, options) {
    options = Object.create(options || null);
    if (!mark.buffer) return null;
    if (!options.maxLength) options.maxLength = 79;
    if (typeof options.indent !== "number") options.indent = 1;
    if (typeof options.linesBefore !== "number") options.linesBefore = 3;
    if (typeof options.linesAfter !== "number") options.linesAfter = 2;
    const re = /\r?\n|\r|\0/g;
    const lineStarts = [0];
    const lineEnds = [];
    let match;
    let foundLineNo = -1;
    while (match = re.exec(mark.buffer)) {
      lineEnds.push(match.index);
      lineStarts.push(match.index + match[0].length);
      if (mark.position <= match.index && foundLineNo < 0) {
        foundLineNo = lineStarts.length - 2;
      }
    }
    if (foundLineNo < 0) foundLineNo = lineStarts.length - 1;
    let result = "";
    const lineNoLength = Math.min(mark.line + options.linesAfter, lineEnds.length).toString().length;
    const maxLineLength = options.maxLength - (options.indent + lineNoLength + 3);
    for (let i = 1; i <= options.linesBefore; i++) {
      if (foundLineNo - i < 0) break;
      const line2 = getLine(
        mark.buffer,
        lineStarts[foundLineNo - i],
        lineEnds[foundLineNo - i],
        mark.position - (lineStarts[foundLineNo] - lineStarts[foundLineNo - i]),
        maxLineLength
      );
      result = common2.repeat(" ", options.indent) + padStart((mark.line - i + 1).toString(), lineNoLength) + " | " + line2.str + "\n" + result;
    }
    const line = getLine(mark.buffer, lineStarts[foundLineNo], lineEnds[foundLineNo], mark.position, maxLineLength);
    result += common2.repeat(" ", options.indent) + padStart((mark.line + 1).toString(), lineNoLength) + " | " + line.str + "\n";
    result += common2.repeat("-", options.indent + lineNoLength + 3 + line.pos) + "^\n";
    for (let i = 1; i <= options.linesAfter; i++) {
      if (foundLineNo + i >= lineEnds.length) break;
      const line2 = getLine(
        mark.buffer,
        lineStarts[foundLineNo + i],
        lineEnds[foundLineNo + i],
        mark.position - (lineStarts[foundLineNo] - lineStarts[foundLineNo + i]),
        maxLineLength
      );
      result += common2.repeat(" ", options.indent) + padStart((mark.line + i + 1).toString(), lineNoLength) + " | " + line2.str + "\n";
    }
    return result.replace(/\n$/, "");
  }
  snippet = makeSnippet;
  return snippet;
}
var type;
var hasRequiredType;
function requireType() {
  if (hasRequiredType) return type;
  hasRequiredType = 1;
  const YAMLException2 = requireException();
  const TYPE_CONSTRUCTOR_OPTIONS = [
    "kind",
    "multi",
    "resolve",
    "construct",
    "instanceOf",
    "predicate",
    "represent",
    "representName",
    "defaultStyle",
    "styleAliases"
  ];
  const YAML_NODE_KINDS = [
    "scalar",
    "sequence",
    "mapping"
  ];
  function compileStyleAliases(map2) {
    const result = {};
    if (map2 !== null) {
      Object.keys(map2).forEach(function(style) {
        map2[style].forEach(function(alias) {
          result[String(alias)] = style;
        });
      });
    }
    return result;
  }
  function Type2(tag, options) {
    options = options || {};
    Object.keys(options).forEach(function(name) {
      if (TYPE_CONSTRUCTOR_OPTIONS.indexOf(name) === -1) {
        throw new YAMLException2('Unknown option "' + name + '" is met in definition of "' + tag + '" YAML type.');
      }
    });
    this.options = options;
    this.tag = tag;
    this.kind = options["kind"] || null;
    this.resolve = options["resolve"] || function() {
      return true;
    };
    this.construct = options["construct"] || function(data) {
      return data;
    };
    this.instanceOf = options["instanceOf"] || null;
    this.predicate = options["predicate"] || null;
    this.represent = options["represent"] || null;
    this.representName = options["representName"] || null;
    this.defaultStyle = options["defaultStyle"] || null;
    this.multi = options["multi"] || false;
    this.styleAliases = compileStyleAliases(options["styleAliases"] || null);
    if (YAML_NODE_KINDS.indexOf(this.kind) === -1) {
      throw new YAMLException2('Unknown kind "' + this.kind + '" is specified for "' + tag + '" YAML type.');
    }
  }
  type = Type2;
  return type;
}
var schema;
var hasRequiredSchema;
function requireSchema() {
  if (hasRequiredSchema) return schema;
  hasRequiredSchema = 1;
  const YAMLException2 = requireException();
  const Type2 = requireType();
  function compileList(schema2, name) {
    const result = [];
    schema2[name].forEach(function(currentType) {
      let newIndex = result.length;
      result.forEach(function(previousType, previousIndex) {
        if (previousType.tag === currentType.tag && previousType.kind === currentType.kind && previousType.multi === currentType.multi) {
          newIndex = previousIndex;
        }
      });
      result[newIndex] = currentType;
    });
    return result;
  }
  function compileMap() {
    const result = {
      scalar: {},
      sequence: {},
      mapping: {},
      fallback: {},
      multi: {
        scalar: [],
        sequence: [],
        mapping: [],
        fallback: []
      }
    };
    function collectType(type2) {
      if (type2.multi) {
        result.multi[type2.kind].push(type2);
        result.multi["fallback"].push(type2);
      } else {
        result[type2.kind][type2.tag] = result["fallback"][type2.tag] = type2;
      }
    }
    for (let index = 0, length = arguments.length; index < length; index += 1) {
      arguments[index].forEach(collectType);
    }
    return result;
  }
  function Schema2(definition) {
    return this.extend(definition);
  }
  Schema2.prototype.extend = function extend(definition) {
    let implicit = [];
    let explicit = [];
    if (definition instanceof Type2) {
      explicit.push(definition);
    } else if (Array.isArray(definition)) {
      explicit = explicit.concat(definition);
    } else if (definition && (Array.isArray(definition.implicit) || Array.isArray(definition.explicit))) {
      if (definition.implicit) implicit = implicit.concat(definition.implicit);
      if (definition.explicit) explicit = explicit.concat(definition.explicit);
    } else {
      throw new YAMLException2("Schema.extend argument should be a Type, [ Type ], or a schema definition ({ implicit: [...], explicit: [...] })");
    }
    implicit.forEach(function(type2) {
      if (!(type2 instanceof Type2)) {
        throw new YAMLException2("Specified list of YAML types (or a single Type object) contains a non-Type object.");
      }
      if (type2.loadKind && type2.loadKind !== "scalar") {
        throw new YAMLException2("There is a non-scalar type in the implicit list of a schema. Implicit resolving of such types is not supported.");
      }
      if (type2.multi) {
        throw new YAMLException2("There is a multi type in the implicit list of a schema. Multi tags can only be listed as explicit.");
      }
    });
    explicit.forEach(function(type2) {
      if (!(type2 instanceof Type2)) {
        throw new YAMLException2("Specified list of YAML types (or a single Type object) contains a non-Type object.");
      }
    });
    const result = Object.create(Schema2.prototype);
    result.implicit = (this.implicit || []).concat(implicit);
    result.explicit = (this.explicit || []).concat(explicit);
    result.compiledImplicit = compileList(result, "implicit");
    result.compiledExplicit = compileList(result, "explicit");
    result.compiledTypeMap = compileMap(result.compiledImplicit, result.compiledExplicit);
    return result;
  };
  schema = Schema2;
  return schema;
}
var str;
var hasRequiredStr;
function requireStr() {
  if (hasRequiredStr) return str;
  hasRequiredStr = 1;
  const Type2 = requireType();
  str = new Type2("tag:yaml.org,2002:str", {
    kind: "scalar",
    construct: function(data) {
      return data !== null ? data : "";
    }
  });
  return str;
}
var seq;
var hasRequiredSeq;
function requireSeq() {
  if (hasRequiredSeq) return seq;
  hasRequiredSeq = 1;
  const Type2 = requireType();
  seq = new Type2("tag:yaml.org,2002:seq", {
    kind: "sequence",
    construct: function(data) {
      return data !== null ? data : [];
    }
  });
  return seq;
}
var map;
var hasRequiredMap;
function requireMap() {
  if (hasRequiredMap) return map;
  hasRequiredMap = 1;
  const Type2 = requireType();
  map = new Type2("tag:yaml.org,2002:map", {
    kind: "mapping",
    construct: function(data) {
      return data !== null ? data : {};
    }
  });
  return map;
}
var failsafe;
var hasRequiredFailsafe;
function requireFailsafe() {
  if (hasRequiredFailsafe) return failsafe;
  hasRequiredFailsafe = 1;
  const Schema2 = requireSchema();
  failsafe = new Schema2({
    explicit: [
      requireStr(),
      requireSeq(),
      requireMap()
    ]
  });
  return failsafe;
}
var _null;
var hasRequired_null;
function require_null() {
  if (hasRequired_null) return _null;
  hasRequired_null = 1;
  const Type2 = requireType();
  function resolveYamlNull(data) {
    if (data === null) return true;
    const max = data.length;
    return max === 1 && data === "~" || max === 4 && (data === "null" || data === "Null" || data === "NULL");
  }
  function constructYamlNull() {
    return null;
  }
  function isNull(object) {
    return object === null;
  }
  _null = new Type2("tag:yaml.org,2002:null", {
    kind: "scalar",
    resolve: resolveYamlNull,
    construct: constructYamlNull,
    predicate: isNull,
    represent: {
      canonical: function() {
        return "~";
      },
      lowercase: function() {
        return "null";
      },
      uppercase: function() {
        return "NULL";
      },
      camelcase: function() {
        return "Null";
      },
      empty: function() {
        return "";
      }
    },
    defaultStyle: "lowercase"
  });
  return _null;
}
var bool;
var hasRequiredBool;
function requireBool() {
  if (hasRequiredBool) return bool;
  hasRequiredBool = 1;
  const Type2 = requireType();
  function resolveYamlBoolean(data) {
    if (data === null) return false;
    const max = data.length;
    return max === 4 && (data === "true" || data === "True" || data === "TRUE") || max === 5 && (data === "false" || data === "False" || data === "FALSE");
  }
  function constructYamlBoolean(data) {
    return data === "true" || data === "True" || data === "TRUE";
  }
  function isBoolean(object) {
    return Object.prototype.toString.call(object) === "[object Boolean]";
  }
  bool = new Type2("tag:yaml.org,2002:bool", {
    kind: "scalar",
    resolve: resolveYamlBoolean,
    construct: constructYamlBoolean,
    predicate: isBoolean,
    represent: {
      lowercase: function(object) {
        return object ? "true" : "false";
      },
      uppercase: function(object) {
        return object ? "TRUE" : "FALSE";
      },
      camelcase: function(object) {
        return object ? "True" : "False";
      }
    },
    defaultStyle: "lowercase"
  });
  return bool;
}
var int;
var hasRequiredInt;
function requireInt() {
  if (hasRequiredInt) return int;
  hasRequiredInt = 1;
  const common2 = requireCommon();
  const Type2 = requireType();
  function isHexCode(c) {
    return c >= 48 && c <= 57 || c >= 65 && c <= 70 || c >= 97 && c <= 102;
  }
  function isOctCode(c) {
    return c >= 48 && c <= 55;
  }
  function isDecCode(c) {
    return c >= 48 && c <= 57;
  }
  function resolveYamlInteger(data) {
    if (data === null) return false;
    const max = data.length;
    let index = 0;
    let hasDigits = false;
    if (!max) return false;
    let ch = data[index];
    if (ch === "-" || ch === "+") {
      ch = data[++index];
    }
    if (ch === "0") {
      if (index + 1 === max) return true;
      ch = data[++index];
      if (ch === "b") {
        index++;
        for (; index < max; index++) {
          ch = data[index];
          if (ch !== "0" && ch !== "1") return false;
          hasDigits = true;
        }
        return hasDigits && isFinite(parseYamlInteger(data));
      }
      if (ch === "x") {
        index++;
        for (; index < max; index++) {
          if (!isHexCode(data.charCodeAt(index))) return false;
          hasDigits = true;
        }
        return hasDigits && isFinite(parseYamlInteger(data));
      }
      if (ch === "o") {
        index++;
        for (; index < max; index++) {
          if (!isOctCode(data.charCodeAt(index))) return false;
          hasDigits = true;
        }
        return hasDigits && isFinite(parseYamlInteger(data));
      }
    }
    for (; index < max; index++) {
      if (!isDecCode(data.charCodeAt(index))) {
        return false;
      }
      hasDigits = true;
    }
    if (!hasDigits) return false;
    return isFinite(parseYamlInteger(data));
  }
  function parseYamlInteger(data) {
    let value = data;
    let sign = 1;
    let ch = value[0];
    if (ch === "-" || ch === "+") {
      if (ch === "-") sign = -1;
      value = value.slice(1);
      ch = value[0];
    }
    if (value === "0") return 0;
    if (ch === "0") {
      if (value[1] === "b") return sign * parseInt(value.slice(2), 2);
      if (value[1] === "x") return sign * parseInt(value.slice(2), 16);
      if (value[1] === "o") return sign * parseInt(value.slice(2), 8);
    }
    return sign * parseInt(value, 10);
  }
  function constructYamlInteger(data) {
    return parseYamlInteger(data);
  }
  function isInteger(object) {
    return Object.prototype.toString.call(object) === "[object Number]" && (object % 1 === 0 && !common2.isNegativeZero(object));
  }
  int = new Type2("tag:yaml.org,2002:int", {
    kind: "scalar",
    resolve: resolveYamlInteger,
    construct: constructYamlInteger,
    predicate: isInteger,
    represent: {
      binary: function(obj) {
        return obj >= 0 ? "0b" + obj.toString(2) : "-0b" + obj.toString(2).slice(1);
      },
      octal: function(obj) {
        return obj >= 0 ? "0o" + obj.toString(8) : "-0o" + obj.toString(8).slice(1);
      },
      decimal: function(obj) {
        return obj.toString(10);
      },
      hexadecimal: function(obj) {
        return obj >= 0 ? "0x" + obj.toString(16).toUpperCase() : "-0x" + obj.toString(16).toUpperCase().slice(1);
      }
    },
    defaultStyle: "decimal",
    styleAliases: {
      binary: [2, "bin"],
      octal: [8, "oct"],
      decimal: [10, "dec"],
      hexadecimal: [16, "hex"]
    }
  });
  return int;
}
var float;
var hasRequiredFloat;
function requireFloat() {
  if (hasRequiredFloat) return float;
  hasRequiredFloat = 1;
  const common2 = requireCommon();
  const Type2 = requireType();
  const YAML_FLOAT_PATTERN = new RegExp(
    // 2.5e4, 2.5 and integers
    "^(?:[-+]?(?:[0-9]+)(?:\\.[0-9]*)?(?:[eE][-+]?[0-9]+)?|\\.[0-9]+(?:[eE][-+]?[0-9]+)?|[-+]?\\.(?:inf|Inf|INF)|\\.(?:nan|NaN|NAN))$"
  );
  const YAML_FLOAT_SPECIAL_PATTERN = new RegExp(
    "^(?:[-+]?\\.(?:inf|Inf|INF)|\\.(?:nan|NaN|NAN))$"
  );
  function resolveYamlFloat(data) {
    if (data === null) return false;
    if (!YAML_FLOAT_PATTERN.test(data)) {
      return false;
    }
    if (isFinite(parseFloat(data, 10))) {
      return true;
    }
    return YAML_FLOAT_SPECIAL_PATTERN.test(data);
  }
  function constructYamlFloat(data) {
    let value = data.toLowerCase();
    const sign = value[0] === "-" ? -1 : 1;
    if ("+-".indexOf(value[0]) >= 0) {
      value = value.slice(1);
    }
    if (value === ".inf") {
      return sign === 1 ? Number.POSITIVE_INFINITY : Number.NEGATIVE_INFINITY;
    } else if (value === ".nan") {
      return NaN;
    }
    return sign * parseFloat(value, 10);
  }
  const SCIENTIFIC_WITHOUT_DOT = /^[-+]?[0-9]+e/;
  function representYamlFloat(object, style) {
    if (isNaN(object)) {
      switch (style) {
        case "lowercase":
          return ".nan";
        case "uppercase":
          return ".NAN";
        case "camelcase":
          return ".NaN";
      }
    } else if (Number.POSITIVE_INFINITY === object) {
      switch (style) {
        case "lowercase":
          return ".inf";
        case "uppercase":
          return ".INF";
        case "camelcase":
          return ".Inf";
      }
    } else if (Number.NEGATIVE_INFINITY === object) {
      switch (style) {
        case "lowercase":
          return "-.inf";
        case "uppercase":
          return "-.INF";
        case "camelcase":
          return "-.Inf";
      }
    } else if (common2.isNegativeZero(object)) {
      return "-0.0";
    }
    const res = object.toString(10);
    return SCIENTIFIC_WITHOUT_DOT.test(res) ? res.replace("e", ".e") : res;
  }
  function isFloat(object) {
    return Object.prototype.toString.call(object) === "[object Number]" && (object % 1 !== 0 || common2.isNegativeZero(object));
  }
  float = new Type2("tag:yaml.org,2002:float", {
    kind: "scalar",
    resolve: resolveYamlFloat,
    construct: constructYamlFloat,
    predicate: isFloat,
    represent: representYamlFloat,
    defaultStyle: "lowercase"
  });
  return float;
}
var json;
var hasRequiredJson;
function requireJson() {
  if (hasRequiredJson) return json;
  hasRequiredJson = 1;
  json = requireFailsafe().extend({
    implicit: [
      require_null(),
      requireBool(),
      requireInt(),
      requireFloat()
    ]
  });
  return json;
}
var core;
var hasRequiredCore;
function requireCore() {
  if (hasRequiredCore) return core;
  hasRequiredCore = 1;
  core = requireJson();
  return core;
}
var timestamp;
var hasRequiredTimestamp;
function requireTimestamp() {
  if (hasRequiredTimestamp) return timestamp;
  hasRequiredTimestamp = 1;
  const Type2 = requireType();
  const YAML_DATE_REGEXP = new RegExp(
    "^([0-9][0-9][0-9][0-9])-([0-9][0-9])-([0-9][0-9])$"
  );
  const YAML_TIMESTAMP_REGEXP = new RegExp(
    "^([0-9][0-9][0-9][0-9])-([0-9][0-9]?)-([0-9][0-9]?)(?:[Tt]|[ \\t]+)([0-9][0-9]?):([0-9][0-9]):([0-9][0-9])(?:\\.([0-9]*))?(?:[ \\t]*(Z|([-+])([0-9][0-9]?)(?::([0-9][0-9]))?))?$"
  );
  function resolveYamlTimestamp(data) {
    if (data === null) return false;
    if (YAML_DATE_REGEXP.exec(data) !== null) return true;
    if (YAML_TIMESTAMP_REGEXP.exec(data) !== null) return true;
    return false;
  }
  function constructYamlTimestamp(data) {
    let fraction = 0;
    let delta = null;
    let match = YAML_DATE_REGEXP.exec(data);
    if (match === null) match = YAML_TIMESTAMP_REGEXP.exec(data);
    if (match === null) throw new Error("Date resolve error");
    const year = +match[1];
    const month = +match[2] - 1;
    const day = +match[3];
    if (!match[4]) {
      return new Date(Date.UTC(year, month, day));
    }
    const hour = +match[4];
    const minute = +match[5];
    const second = +match[6];
    if (match[7]) {
      fraction = match[7].slice(0, 3);
      while (fraction.length < 3) {
        fraction += "0";
      }
      fraction = +fraction;
    }
    if (match[9]) {
      const tzHour = +match[10];
      const tzMinute = +(match[11] || 0);
      delta = (tzHour * 60 + tzMinute) * 6e4;
      if (match[9] === "-") delta = -delta;
    }
    const date = new Date(Date.UTC(year, month, day, hour, minute, second, fraction));
    if (delta) date.setTime(date.getTime() - delta);
    return date;
  }
  function representYamlTimestamp(object) {
    return object.toISOString();
  }
  timestamp = new Type2("tag:yaml.org,2002:timestamp", {
    kind: "scalar",
    resolve: resolveYamlTimestamp,
    construct: constructYamlTimestamp,
    instanceOf: Date,
    represent: representYamlTimestamp
  });
  return timestamp;
}
var merge;
var hasRequiredMerge;
function requireMerge() {
  if (hasRequiredMerge) return merge;
  hasRequiredMerge = 1;
  const Type2 = requireType();
  function resolveYamlMerge(data) {
    return data === "<<" || data === null;
  }
  merge = new Type2("tag:yaml.org,2002:merge", {
    kind: "scalar",
    resolve: resolveYamlMerge
  });
  return merge;
}
var binary;
var hasRequiredBinary;
function requireBinary() {
  if (hasRequiredBinary) return binary;
  hasRequiredBinary = 1;
  const Type2 = requireType();
  const BASE64_MAP = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=\n\r";
  function resolveYamlBinary(data) {
    if (data === null) return false;
    let bitlen = 0;
    const max = data.length;
    const map2 = BASE64_MAP;
    for (let idx = 0; idx < max; idx++) {
      const code = map2.indexOf(data.charAt(idx));
      if (code > 64) continue;
      if (code < 0) return false;
      bitlen += 6;
    }
    return bitlen % 8 === 0;
  }
  function constructYamlBinary(data) {
    const input = data.replace(/[\r\n=]/g, "");
    const max = input.length;
    const map2 = BASE64_MAP;
    let bits = 0;
    const result = [];
    for (let idx = 0; idx < max; idx++) {
      if (idx % 4 === 0 && idx) {
        result.push(bits >> 16 & 255);
        result.push(bits >> 8 & 255);
        result.push(bits & 255);
      }
      bits = bits << 6 | map2.indexOf(input.charAt(idx));
    }
    const tailbits = max % 4 * 6;
    if (tailbits === 0) {
      result.push(bits >> 16 & 255);
      result.push(bits >> 8 & 255);
      result.push(bits & 255);
    } else if (tailbits === 18) {
      result.push(bits >> 10 & 255);
      result.push(bits >> 2 & 255);
    } else if (tailbits === 12) {
      result.push(bits >> 4 & 255);
    }
    return new Uint8Array(result);
  }
  function representYamlBinary(object) {
    let result = "";
    let bits = 0;
    const max = object.length;
    const map2 = BASE64_MAP;
    for (let idx = 0; idx < max; idx++) {
      if (idx % 3 === 0 && idx) {
        result += map2[bits >> 18 & 63];
        result += map2[bits >> 12 & 63];
        result += map2[bits >> 6 & 63];
        result += map2[bits & 63];
      }
      bits = (bits << 8) + object[idx];
    }
    const tail = max % 3;
    if (tail === 0) {
      result += map2[bits >> 18 & 63];
      result += map2[bits >> 12 & 63];
      result += map2[bits >> 6 & 63];
      result += map2[bits & 63];
    } else if (tail === 2) {
      result += map2[bits >> 10 & 63];
      result += map2[bits >> 4 & 63];
      result += map2[bits << 2 & 63];
      result += map2[64];
    } else if (tail === 1) {
      result += map2[bits >> 2 & 63];
      result += map2[bits << 4 & 63];
      result += map2[64];
      result += map2[64];
    }
    return result;
  }
  function isBinary(obj) {
    return Object.prototype.toString.call(obj) === "[object Uint8Array]";
  }
  binary = new Type2("tag:yaml.org,2002:binary", {
    kind: "scalar",
    resolve: resolveYamlBinary,
    construct: constructYamlBinary,
    predicate: isBinary,
    represent: representYamlBinary
  });
  return binary;
}
var omap;
var hasRequiredOmap;
function requireOmap() {
  if (hasRequiredOmap) return omap;
  hasRequiredOmap = 1;
  const Type2 = requireType();
  const _hasOwnProperty = Object.prototype.hasOwnProperty;
  const _toString = Object.prototype.toString;
  function resolveYamlOmap(data) {
    if (data === null) return true;
    const objectKeys = [];
    const object = data;
    for (let index = 0, length = object.length; index < length; index += 1) {
      const pair = object[index];
      let pairHasKey = false;
      if (_toString.call(pair) !== "[object Object]") return false;
      let pairKey2;
      for (pairKey2 in pair) {
        if (_hasOwnProperty.call(pair, pairKey2)) {
          if (!pairHasKey) pairHasKey = true;
          else return false;
        }
      }
      if (!pairHasKey) return false;
      if (objectKeys.indexOf(pairKey2) === -1) objectKeys.push(pairKey2);
      else return false;
    }
    return true;
  }
  function constructYamlOmap(data) {
    return data !== null ? data : [];
  }
  omap = new Type2("tag:yaml.org,2002:omap", {
    kind: "sequence",
    resolve: resolveYamlOmap,
    construct: constructYamlOmap
  });
  return omap;
}
var pairs;
var hasRequiredPairs;
function requirePairs() {
  if (hasRequiredPairs) return pairs;
  hasRequiredPairs = 1;
  const Type2 = requireType();
  const _toString = Object.prototype.toString;
  function resolveYamlPairs(data) {
    if (data === null) return true;
    const object = data;
    const result = new Array(object.length);
    for (let index = 0, length = object.length; index < length; index += 1) {
      const pair = object[index];
      if (_toString.call(pair) !== "[object Object]") return false;
      const keys = Object.keys(pair);
      if (keys.length !== 1) return false;
      result[index] = [keys[0], pair[keys[0]]];
    }
    return true;
  }
  function constructYamlPairs(data) {
    if (data === null) return [];
    const object = data;
    const result = new Array(object.length);
    for (let index = 0, length = object.length; index < length; index += 1) {
      const pair = object[index];
      const keys = Object.keys(pair);
      result[index] = [keys[0], pair[keys[0]]];
    }
    return result;
  }
  pairs = new Type2("tag:yaml.org,2002:pairs", {
    kind: "sequence",
    resolve: resolveYamlPairs,
    construct: constructYamlPairs
  });
  return pairs;
}
var set;
var hasRequiredSet;
function requireSet() {
  if (hasRequiredSet) return set;
  hasRequiredSet = 1;
  const Type2 = requireType();
  const _hasOwnProperty = Object.prototype.hasOwnProperty;
  function resolveYamlSet(data) {
    if (data === null) return true;
    const object = data;
    for (const key in object) {
      if (_hasOwnProperty.call(object, key)) {
        if (object[key] !== null) return false;
      }
    }
    return true;
  }
  function constructYamlSet(data) {
    return data !== null ? data : {};
  }
  set = new Type2("tag:yaml.org,2002:set", {
    kind: "mapping",
    resolve: resolveYamlSet,
    construct: constructYamlSet
  });
  return set;
}
var _default;
var hasRequired_default;
function require_default() {
  if (hasRequired_default) return _default;
  hasRequired_default = 1;
  _default = requireCore().extend({
    implicit: [
      requireTimestamp(),
      requireMerge()
    ],
    explicit: [
      requireBinary(),
      requireOmap(),
      requirePairs(),
      requireSet()
    ]
  });
  return _default;
}
var hasRequiredLoader;
function requireLoader() {
  if (hasRequiredLoader) return loader;
  hasRequiredLoader = 1;
  const common2 = requireCommon();
  const YAMLException2 = requireException();
  const makeSnippet = requireSnippet();
  const DEFAULT_SCHEMA2 = require_default();
  const _hasOwnProperty = Object.prototype.hasOwnProperty;
  const CONTEXT_FLOW_IN = 1;
  const CONTEXT_FLOW_OUT = 2;
  const CONTEXT_BLOCK_IN = 3;
  const CONTEXT_BLOCK_OUT = 4;
  const CHOMPING_CLIP = 1;
  const CHOMPING_STRIP = 2;
  const CHOMPING_KEEP = 3;
  const PATTERN_NON_PRINTABLE = /[\x00-\x08\x0B\x0C\x0E-\x1F\x7F-\x84\x86-\x9F\uFFFE\uFFFF]|[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?:[^\uD800-\uDBFF]|^)[\uDC00-\uDFFF]/;
  const PATTERN_NON_ASCII_LINE_BREAKS = /[\x85\u2028\u2029]/;
  const PATTERN_FLOW_INDICATORS = /[,\[\]{}]/;
  const PATTERN_TAG_HANDLE = /^(?:!|!!|![0-9A-Za-z-]+!)$/;
  const PATTERN_TAG_URI = /^(?:!|[^,\[\]{}])(?:%[0-9a-f]{2}|[0-9a-z\-#;/?:@&=+$,_.!~*'()\[\]])*$/i;
  function _class(obj) {
    return Object.prototype.toString.call(obj);
  }
  function isEol(c) {
    return c === 10 || c === 13;
  }
  function isWhiteSpace(c) {
    return c === 9 || c === 32;
  }
  function isWsOrEol(c) {
    return c === 9 || c === 32 || c === 10 || c === 13;
  }
  function isFlowIndicator(c) {
    return c === 44 || c === 91 || c === 93 || c === 123 || c === 125;
  }
  function fromHexCode(c) {
    if (c >= 48 && c <= 57) {
      return c - 48;
    }
    const lc = c | 32;
    if (lc >= 97 && lc <= 102) {
      return lc - 97 + 10;
    }
    return -1;
  }
  function escapedHexLen(c) {
    if (c === 120) {
      return 2;
    }
    if (c === 117) {
      return 4;
    }
    if (c === 85) {
      return 8;
    }
    return 0;
  }
  function fromDecimalCode(c) {
    if (c >= 48 && c <= 57) {
      return c - 48;
    }
    return -1;
  }
  function simpleEscapeSequence(c) {
    switch (c) {
      case 48:
        return "\0";
      case 97:
        return "\x07";
      case 98:
        return "\b";
      case 116:
        return "	";
      case 9:
        return "	";
      case 110:
        return "\n";
      case 118:
        return "\v";
      case 102:
        return "\f";
      case 114:
        return "\r";
      case 101:
        return "\x1B";
      case 32:
        return " ";
      case 34:
        return '"';
      case 47:
        return "/";
      case 92:
        return "\\";
      case 78:
        return "\x85";
      case 95:
        return "\xA0";
      case 76:
        return "\u2028";
      case 80:
        return "\u2029";
      default:
        return "";
    }
  }
  function charFromCodepoint(c) {
    if (c <= 65535) {
      return String.fromCharCode(c);
    }
    return String.fromCharCode(
      (c - 65536 >> 10) + 55296,
      (c - 65536 & 1023) + 56320
    );
  }
  function setProperty(object, key, value) {
    if (key === "__proto__") {
      Object.defineProperty(object, key, {
        configurable: true,
        enumerable: true,
        writable: true,
        value
      });
    } else {
      object[key] = value;
    }
  }
  const simpleEscapeCheck = new Array(256);
  const simpleEscapeMap = new Array(256);
  for (let i = 0; i < 256; i++) {
    simpleEscapeCheck[i] = simpleEscapeSequence(i) ? 1 : 0;
    simpleEscapeMap[i] = simpleEscapeSequence(i);
  }
  function State(input, options) {
    this.input = input;
    this.filename = options["filename"] || null;
    this.schema = options["schema"] || DEFAULT_SCHEMA2;
    this.onWarning = options["onWarning"] || null;
    this.legacy = options["legacy"] || false;
    this.json = options["json"] || false;
    this.listener = options["listener"] || null;
    this.maxDepth = typeof options["maxDepth"] === "number" ? options["maxDepth"] : 100;
    this.maxTotalMergeKeys = typeof options["maxTotalMergeKeys"] === "number" ? options["maxTotalMergeKeys"] : 1e4;
    this.implicitTypes = this.schema.compiledImplicit;
    this.typeMap = this.schema.compiledTypeMap;
    this.length = input.length;
    this.position = 0;
    this.line = 0;
    this.lineStart = 0;
    this.lineIndent = 0;
    this.depth = 0;
    this.totalMergeKeys = 0;
    this.firstTabInLine = -1;
    this.documents = [];
    this.anchorMapTransactions = [];
  }
  function generateError(state, message) {
    const mark = {
      name: state.filename,
      buffer: state.input.slice(0, -1),
      // omit trailing \0
      position: state.position,
      line: state.line,
      column: state.position - state.lineStart
    };
    mark.snippet = makeSnippet(mark);
    return new YAMLException2(message, mark);
  }
  function throwError(state, message) {
    throw generateError(state, message);
  }
  function throwWarning(state, message) {
    if (state.onWarning) {
      state.onWarning.call(null, generateError(state, message));
    }
  }
  function storeAnchor(state, name, value) {
    const transactions = state.anchorMapTransactions;
    if (transactions.length !== 0) {
      const transaction = transactions[transactions.length - 1];
      if (!_hasOwnProperty.call(transaction, name)) {
        transaction[name] = {
          existed: _hasOwnProperty.call(state.anchorMap, name),
          value: state.anchorMap[name]
        };
      }
    }
    state.anchorMap[name] = value;
  }
  function beginAnchorTransaction(state) {
    state.anchorMapTransactions.push(/* @__PURE__ */ Object.create(null));
  }
  function commitAnchorTransaction(state) {
    const transaction = state.anchorMapTransactions.pop();
    const transactions = state.anchorMapTransactions;
    if (transactions.length === 0) return;
    const parent = transactions[transactions.length - 1];
    const names = Object.keys(transaction);
    for (let index = 0, length = names.length; index < length; index += 1) {
      const name = names[index];
      if (!_hasOwnProperty.call(parent, name)) {
        parent[name] = transaction[name];
      }
    }
  }
  function rollbackAnchorTransaction(state) {
    const transaction = state.anchorMapTransactions.pop();
    const names = Object.keys(transaction);
    for (let index = names.length - 1; index >= 0; index -= 1) {
      const entry = transaction[names[index]];
      if (entry.existed) {
        state.anchorMap[names[index]] = entry.value;
      } else {
        delete state.anchorMap[names[index]];
      }
    }
  }
  function snapshotState(state) {
    return {
      position: state.position,
      line: state.line,
      lineStart: state.lineStart,
      lineIndent: state.lineIndent,
      firstTabInLine: state.firstTabInLine,
      tag: state.tag,
      anchor: state.anchor,
      kind: state.kind,
      result: state.result
    };
  }
  function restoreState(state, snapshot) {
    state.position = snapshot.position;
    state.line = snapshot.line;
    state.lineStart = snapshot.lineStart;
    state.lineIndent = snapshot.lineIndent;
    state.firstTabInLine = snapshot.firstTabInLine;
    state.tag = snapshot.tag;
    state.anchor = snapshot.anchor;
    state.kind = snapshot.kind;
    state.result = snapshot.result;
  }
  const directiveHandlers = {
    YAML: function handleYamlDirective(state, name, args) {
      if (state.version !== null) {
        throwError(state, "duplication of %YAML directive");
      }
      if (args.length !== 1) {
        throwError(state, "YAML directive accepts exactly one argument");
      }
      const match = /^([0-9]+)\.([0-9]+)$/.exec(args[0]);
      if (match === null) {
        throwError(state, "ill-formed argument of the YAML directive");
      }
      const major = parseInt(match[1], 10);
      const minor = parseInt(match[2], 10);
      if (major !== 1) {
        throwError(state, "unacceptable YAML version of the document");
      }
      state.version = args[0];
      state.checkLineBreaks = minor < 2;
      if (minor !== 1 && minor !== 2) {
        throwWarning(state, "unsupported YAML version of the document");
      }
    },
    TAG: function handleTagDirective(state, name, args) {
      let prefix;
      if (args.length !== 2) {
        throwError(state, "TAG directive accepts exactly two arguments");
      }
      const handle = args[0];
      prefix = args[1];
      if (!PATTERN_TAG_HANDLE.test(handle)) {
        throwError(state, "ill-formed tag handle (first argument) of the TAG directive");
      }
      if (_hasOwnProperty.call(state.tagMap, handle)) {
        throwError(state, 'there is a previously declared suffix for "' + handle + '" tag handle');
      }
      if (!PATTERN_TAG_URI.test(prefix)) {
        throwError(state, "ill-formed tag prefix (second argument) of the TAG directive");
      }
      try {
        prefix = decodeURIComponent(prefix);
      } catch (err) {
        throwError(state, "tag prefix is malformed: " + prefix);
      }
      state.tagMap[handle] = prefix;
    }
  };
  function captureSegment(state, start, end, checkJson) {
    if (start < end) {
      const _result = state.input.slice(start, end);
      if (checkJson) {
        for (let _position = 0, _length = _result.length; _position < _length; _position += 1) {
          const _character = _result.charCodeAt(_position);
          if (!(_character === 9 || _character >= 32 && _character <= 1114111)) {
            throwError(state, "expected valid JSON character");
          }
        }
      } else if (PATTERN_NON_PRINTABLE.test(_result)) {
        throwError(state, "the stream contains non-printable characters");
      }
      state.result += _result;
    }
  }
  function mergeMappings(state, destination, source, overridableKeys) {
    if (!common2.isObject(source)) {
      throwError(state, "cannot merge mappings; the provided source object is unacceptable");
    }
    const sourceKeys = Object.keys(source);
    for (let index = 0, quantity = sourceKeys.length; index < quantity; index += 1) {
      const key = sourceKeys[index];
      if (state.maxTotalMergeKeys !== -1 && ++state.totalMergeKeys > state.maxTotalMergeKeys) {
        throwError(state, "merge keys exceeded maxTotalMergeKeys (" + state.maxTotalMergeKeys + ")");
      }
      if (!_hasOwnProperty.call(destination, key)) {
        setProperty(destination, key, source[key]);
        overridableKeys[key] = true;
      }
    }
  }
  function storeMappingPair(state, _result, overridableKeys, keyTag, keyNode, valueNode, startLine, startLineStart, startPos) {
    if (Array.isArray(keyNode)) {
      keyNode = Array.prototype.slice.call(keyNode);
      for (let index = 0, quantity = keyNode.length; index < quantity; index += 1) {
        if (Array.isArray(keyNode[index])) {
          throwError(state, "nested arrays are not supported inside keys");
        }
        if (typeof keyNode === "object" && _class(keyNode[index]) === "[object Object]") {
          keyNode[index] = "[object Object]";
        }
      }
    }
    if (typeof keyNode === "object" && _class(keyNode) === "[object Object]") {
      keyNode = "[object Object]";
    }
    keyNode = String(keyNode);
    if (_result === null) {
      _result = {};
    }
    if (keyTag === "tag:yaml.org,2002:merge") {
      if (Array.isArray(valueNode)) {
        for (let index = 0, quantity = valueNode.length; index < quantity; index += 1) {
          mergeMappings(state, _result, valueNode[index], overridableKeys);
        }
      } else {
        mergeMappings(state, _result, valueNode, overridableKeys);
      }
    } else {
      if (!state.json && !_hasOwnProperty.call(overridableKeys, keyNode) && _hasOwnProperty.call(_result, keyNode)) {
        state.line = startLine || state.line;
        state.lineStart = startLineStart || state.lineStart;
        state.position = startPos || state.position;
        throwError(state, "duplicated mapping key");
      }
      setProperty(_result, keyNode, valueNode);
      delete overridableKeys[keyNode];
    }
    return _result;
  }
  function readLineBreak(state) {
    const ch = state.input.charCodeAt(state.position);
    if (ch === 10) {
      state.position++;
    } else if (ch === 13) {
      state.position++;
      if (state.input.charCodeAt(state.position) === 10) {
        state.position++;
      }
    } else {
      throwError(state, "a line break is expected");
    }
    state.line += 1;
    state.lineStart = state.position;
    state.firstTabInLine = -1;
  }
  function skipSeparationSpace(state, allowComments, checkIndent) {
    let lineBreaks = 0;
    let ch = state.input.charCodeAt(state.position);
    while (ch !== 0) {
      while (isWhiteSpace(ch)) {
        if (ch === 9 && state.firstTabInLine === -1) {
          state.firstTabInLine = state.position;
        }
        ch = state.input.charCodeAt(++state.position);
      }
      if (allowComments && ch === 35) {
        do {
          ch = state.input.charCodeAt(++state.position);
        } while (ch !== 10 && ch !== 13 && ch !== 0);
      }
      if (isEol(ch)) {
        readLineBreak(state);
        ch = state.input.charCodeAt(state.position);
        lineBreaks++;
        state.lineIndent = 0;
        while (ch === 32) {
          state.lineIndent++;
          ch = state.input.charCodeAt(++state.position);
        }
      } else {
        break;
      }
    }
    if (checkIndent !== -1 && lineBreaks !== 0 && state.lineIndent < checkIndent) {
      throwWarning(state, "deficient indentation");
    }
    return lineBreaks;
  }
  function testDocumentSeparator(state) {
    let _position = state.position;
    let ch = state.input.charCodeAt(_position);
    if ((ch === 45 || ch === 46) && ch === state.input.charCodeAt(_position + 1) && ch === state.input.charCodeAt(_position + 2)) {
      _position += 3;
      ch = state.input.charCodeAt(_position);
      if (ch === 0 || isWsOrEol(ch)) {
        return true;
      }
    }
    return false;
  }
  function writeFoldedLines(state, count) {
    if (count === 1) {
      state.result += " ";
    } else if (count > 1) {
      state.result += common2.repeat("\n", count - 1);
    }
  }
  function readPlainScalar(state, nodeIndent, withinFlowCollection) {
    let captureStart;
    let captureEnd;
    let hasPendingContent;
    let _line;
    let _lineStart;
    let _lineIndent;
    const _kind = state.kind;
    const _result = state.result;
    let ch = state.input.charCodeAt(state.position);
    if (isWsOrEol(ch) || isFlowIndicator(ch) || ch === 35 || ch === 38 || ch === 42 || ch === 33 || ch === 124 || ch === 62 || ch === 39 || ch === 34 || ch === 37 || ch === 64 || ch === 96) {
      return false;
    }
    if (ch === 63 || ch === 45) {
      const following = state.input.charCodeAt(state.position + 1);
      if (isWsOrEol(following) || withinFlowCollection && isFlowIndicator(following)) {
        return false;
      }
    }
    state.kind = "scalar";
    state.result = "";
    captureStart = captureEnd = state.position;
    hasPendingContent = false;
    while (ch !== 0) {
      if (ch === 58) {
        const following = state.input.charCodeAt(state.position + 1);
        if (isWsOrEol(following) || withinFlowCollection && isFlowIndicator(following)) {
          break;
        }
      } else if (ch === 35) {
        const preceding = state.input.charCodeAt(state.position - 1);
        if (isWsOrEol(preceding)) {
          break;
        }
      } else if (state.position === state.lineStart && testDocumentSeparator(state) || withinFlowCollection && isFlowIndicator(ch)) {
        break;
      } else if (isEol(ch)) {
        _line = state.line;
        _lineStart = state.lineStart;
        _lineIndent = state.lineIndent;
        skipSeparationSpace(state, false, -1);
        if (state.lineIndent >= nodeIndent) {
          hasPendingContent = true;
          ch = state.input.charCodeAt(state.position);
          continue;
        } else {
          state.position = captureEnd;
          state.line = _line;
          state.lineStart = _lineStart;
          state.lineIndent = _lineIndent;
          break;
        }
      }
      if (hasPendingContent) {
        captureSegment(state, captureStart, captureEnd, false);
        writeFoldedLines(state, state.line - _line);
        captureStart = captureEnd = state.position;
        hasPendingContent = false;
      }
      if (!isWhiteSpace(ch)) {
        captureEnd = state.position + 1;
      }
      ch = state.input.charCodeAt(++state.position);
    }
    captureSegment(state, captureStart, captureEnd, false);
    if (state.result) {
      return true;
    }
    state.kind = _kind;
    state.result = _result;
    return false;
  }
  function readSingleQuotedScalar(state, nodeIndent) {
    let captureStart;
    let captureEnd;
    let ch = state.input.charCodeAt(state.position);
    if (ch !== 39) {
      return false;
    }
    state.kind = "scalar";
    state.result = "";
    state.position++;
    captureStart = captureEnd = state.position;
    while ((ch = state.input.charCodeAt(state.position)) !== 0) {
      if (ch === 39) {
        captureSegment(state, captureStart, state.position, true);
        ch = state.input.charCodeAt(++state.position);
        if (ch === 39) {
          captureStart = state.position;
          state.position++;
          captureEnd = state.position;
        } else {
          return true;
        }
      } else if (isEol(ch)) {
        captureSegment(state, captureStart, captureEnd, true);
        writeFoldedLines(state, skipSeparationSpace(state, false, nodeIndent));
        captureStart = captureEnd = state.position;
      } else if (state.position === state.lineStart && testDocumentSeparator(state)) {
        throwError(state, "unexpected end of the document within a single quoted scalar");
      } else {
        state.position++;
        if (!isWhiteSpace(ch)) {
          captureEnd = state.position;
        }
      }
    }
    throwError(state, "unexpected end of the stream within a single quoted scalar");
  }
  function readDoubleQuotedScalar(state, nodeIndent) {
    let captureStart;
    let captureEnd;
    let tmp;
    let ch = state.input.charCodeAt(state.position);
    if (ch !== 34) {
      return false;
    }
    state.kind = "scalar";
    state.result = "";
    state.position++;
    captureStart = captureEnd = state.position;
    while ((ch = state.input.charCodeAt(state.position)) !== 0) {
      if (ch === 34) {
        captureSegment(state, captureStart, state.position, true);
        state.position++;
        return true;
      } else if (ch === 92) {
        captureSegment(state, captureStart, state.position, true);
        ch = state.input.charCodeAt(++state.position);
        if (isEol(ch)) {
          skipSeparationSpace(state, false, nodeIndent);
        } else if (ch < 256 && simpleEscapeCheck[ch]) {
          state.result += simpleEscapeMap[ch];
          state.position++;
        } else if ((tmp = escapedHexLen(ch)) > 0) {
          let hexLength = tmp;
          let hexResult = 0;
          for (; hexLength > 0; hexLength--) {
            ch = state.input.charCodeAt(++state.position);
            if ((tmp = fromHexCode(ch)) >= 0) {
              hexResult = (hexResult << 4) + tmp;
            } else {
              throwError(state, "expected hexadecimal character");
            }
          }
          state.result += charFromCodepoint(hexResult);
          state.position++;
        } else {
          throwError(state, "unknown escape sequence");
        }
        captureStart = captureEnd = state.position;
      } else if (isEol(ch)) {
        captureSegment(state, captureStart, captureEnd, true);
        writeFoldedLines(state, skipSeparationSpace(state, false, nodeIndent));
        captureStart = captureEnd = state.position;
      } else if (state.position === state.lineStart && testDocumentSeparator(state)) {
        throwError(state, "unexpected end of the document within a double quoted scalar");
      } else {
        state.position++;
        if (!isWhiteSpace(ch)) {
          captureEnd = state.position;
        }
      }
    }
    throwError(state, "unexpected end of the stream within a double quoted scalar");
  }
  function readFlowCollection(state, nodeIndent) {
    let readNext = true;
    let _line;
    let _lineStart;
    let _pos;
    const _tag = state.tag;
    let _result;
    const _anchor = state.anchor;
    let terminator;
    let isPair;
    let isExplicitPair;
    let isMapping;
    const overridableKeys = /* @__PURE__ */ Object.create(null);
    let keyNode;
    let keyTag;
    let valueNode;
    let ch = state.input.charCodeAt(state.position);
    if (ch === 91) {
      terminator = 93;
      isMapping = false;
      _result = [];
    } else if (ch === 123) {
      terminator = 125;
      isMapping = true;
      _result = {};
    } else {
      return false;
    }
    if (state.anchor !== null) {
      storeAnchor(state, state.anchor, _result);
    }
    ch = state.input.charCodeAt(++state.position);
    while (ch !== 0) {
      skipSeparationSpace(state, true, nodeIndent);
      ch = state.input.charCodeAt(state.position);
      if (ch === terminator) {
        state.position++;
        state.tag = _tag;
        state.anchor = _anchor;
        state.kind = isMapping ? "mapping" : "sequence";
        state.result = _result;
        return true;
      } else if (!readNext) {
        throwError(state, "missed comma between flow collection entries");
      } else if (ch === 44) {
        throwError(state, "expected the node content, but found ','");
      }
      keyTag = keyNode = valueNode = null;
      isPair = isExplicitPair = false;
      if (ch === 63) {
        const following = state.input.charCodeAt(state.position + 1);
        if (isWsOrEol(following)) {
          isPair = isExplicitPair = true;
          state.position++;
          skipSeparationSpace(state, true, nodeIndent);
        }
      }
      _line = state.line;
      _lineStart = state.lineStart;
      _pos = state.position;
      composeNode(state, nodeIndent, CONTEXT_FLOW_IN, false, true);
      keyTag = state.tag;
      keyNode = state.result;
      skipSeparationSpace(state, true, nodeIndent);
      ch = state.input.charCodeAt(state.position);
      if ((isExplicitPair || state.line === _line) && ch === 58) {
        isPair = true;
        ch = state.input.charCodeAt(++state.position);
        skipSeparationSpace(state, true, nodeIndent);
        composeNode(state, nodeIndent, CONTEXT_FLOW_IN, false, true);
        valueNode = state.result;
      }
      if (isMapping) {
        storeMappingPair(state, _result, overridableKeys, keyTag, keyNode, valueNode, _line, _lineStart, _pos);
      } else if (isPair) {
        _result.push(storeMappingPair(state, null, overridableKeys, keyTag, keyNode, valueNode, _line, _lineStart, _pos));
      } else {
        _result.push(keyNode);
      }
      skipSeparationSpace(state, true, nodeIndent);
      ch = state.input.charCodeAt(state.position);
      if (ch === 44) {
        readNext = true;
        ch = state.input.charCodeAt(++state.position);
      } else {
        readNext = false;
      }
    }
    throwError(state, "unexpected end of the stream within a flow collection");
  }
  function readBlockScalar(state, nodeIndent) {
    let folding;
    let chomping = CHOMPING_CLIP;
    let didReadContent = false;
    let detectedIndent = false;
    let textIndent = nodeIndent;
    let emptyLines = 0;
    let atMoreIndented = false;
    let tmp;
    let ch = state.input.charCodeAt(state.position);
    if (ch === 124) {
      folding = false;
    } else if (ch === 62) {
      folding = true;
    } else {
      return false;
    }
    state.kind = "scalar";
    state.result = "";
    while (ch !== 0) {
      ch = state.input.charCodeAt(++state.position);
      if (ch === 43 || ch === 45) {
        if (CHOMPING_CLIP === chomping) {
          chomping = ch === 43 ? CHOMPING_KEEP : CHOMPING_STRIP;
        } else {
          throwError(state, "repeat of a chomping mode identifier");
        }
      } else if ((tmp = fromDecimalCode(ch)) >= 0) {
        if (tmp === 0) {
          throwError(state, "bad explicit indentation width of a block scalar; it cannot be less than one");
        } else if (!detectedIndent) {
          textIndent = nodeIndent + tmp - 1;
          detectedIndent = true;
        } else {
          throwError(state, "repeat of an indentation width identifier");
        }
      } else {
        break;
      }
    }
    if (isWhiteSpace(ch)) {
      do {
        ch = state.input.charCodeAt(++state.position);
      } while (isWhiteSpace(ch));
      if (ch === 35) {
        do {
          ch = state.input.charCodeAt(++state.position);
        } while (!isEol(ch) && ch !== 0);
      }
    }
    while (ch !== 0) {
      readLineBreak(state);
      state.lineIndent = 0;
      ch = state.input.charCodeAt(state.position);
      while ((!detectedIndent || state.lineIndent < textIndent) && ch === 32) {
        state.lineIndent++;
        ch = state.input.charCodeAt(++state.position);
      }
      if (!detectedIndent && state.lineIndent > textIndent) {
        textIndent = state.lineIndent;
      }
      if (isEol(ch)) {
        emptyLines++;
        continue;
      }
      if (!detectedIndent && textIndent === 0) {
        throwError(state, "missing indentation for block scalar");
      }
      if (state.lineIndent < textIndent) {
        if (chomping === CHOMPING_KEEP) {
          state.result += common2.repeat("\n", didReadContent ? 1 + emptyLines : emptyLines);
        } else if (chomping === CHOMPING_CLIP) {
          if (didReadContent) {
            state.result += "\n";
          }
        }
        break;
      }
      if (folding) {
        if (isWhiteSpace(ch)) {
          atMoreIndented = true;
          state.result += common2.repeat("\n", didReadContent ? 1 + emptyLines : emptyLines);
        } else if (atMoreIndented) {
          atMoreIndented = false;
          state.result += common2.repeat("\n", emptyLines + 1);
        } else if (emptyLines === 0) {
          if (didReadContent) {
            state.result += " ";
          }
        } else {
          state.result += common2.repeat("\n", emptyLines);
        }
      } else {
        state.result += common2.repeat("\n", didReadContent ? 1 + emptyLines : emptyLines);
      }
      didReadContent = true;
      detectedIndent = true;
      emptyLines = 0;
      const captureStart = state.position;
      while (!isEol(ch) && ch !== 0) {
        ch = state.input.charCodeAt(++state.position);
      }
      captureSegment(state, captureStart, state.position, false);
    }
    return true;
  }
  function readBlockSequence(state, nodeIndent) {
    const _tag = state.tag;
    const _anchor = state.anchor;
    const _result = [];
    let detected = false;
    if (state.firstTabInLine !== -1) return false;
    if (state.anchor !== null) {
      storeAnchor(state, state.anchor, _result);
    }
    let ch = state.input.charCodeAt(state.position);
    while (ch !== 0) {
      if (state.firstTabInLine !== -1) {
        state.position = state.firstTabInLine;
        throwError(state, "tab characters must not be used in indentation");
      }
      if (ch !== 45) {
        break;
      }
      const following = state.input.charCodeAt(state.position + 1);
      if (!isWsOrEol(following)) {
        break;
      }
      detected = true;
      state.position++;
      if (skipSeparationSpace(state, true, -1)) {
        if (state.lineIndent <= nodeIndent) {
          _result.push(null);
          ch = state.input.charCodeAt(state.position);
          continue;
        }
      }
      const _line = state.line;
      composeNode(state, nodeIndent, CONTEXT_BLOCK_IN, false, true);
      _result.push(state.result);
      skipSeparationSpace(state, true, -1);
      ch = state.input.charCodeAt(state.position);
      if ((state.line === _line || state.lineIndent > nodeIndent) && ch !== 0) {
        throwError(state, "bad indentation of a sequence entry");
      } else if (state.lineIndent < nodeIndent) {
        break;
      }
    }
    if (detected) {
      state.tag = _tag;
      state.anchor = _anchor;
      state.kind = "sequence";
      state.result = _result;
      return true;
    }
    return false;
  }
  function readBlockMapping(state, nodeIndent, flowIndent) {
    let allowCompact;
    let _keyLine;
    let _keyLineStart;
    let _keyPos;
    const _tag = state.tag;
    const _anchor = state.anchor;
    const _result = {};
    const overridableKeys = /* @__PURE__ */ Object.create(null);
    let keyTag = null;
    let keyNode = null;
    let valueNode = null;
    let atExplicitKey = false;
    let detected = false;
    if (state.firstTabInLine !== -1) return false;
    if (state.anchor !== null) {
      storeAnchor(state, state.anchor, _result);
    }
    let ch = state.input.charCodeAt(state.position);
    while (ch !== 0) {
      if (!atExplicitKey && state.firstTabInLine !== -1) {
        state.position = state.firstTabInLine;
        throwError(state, "tab characters must not be used in indentation");
      }
      const following = state.input.charCodeAt(state.position + 1);
      const _line = state.line;
      if ((ch === 63 || ch === 58) && isWsOrEol(following)) {
        if (ch === 63) {
          if (atExplicitKey) {
            storeMappingPair(state, _result, overridableKeys, keyTag, keyNode, null, _keyLine, _keyLineStart, _keyPos);
            keyTag = keyNode = valueNode = null;
          }
          detected = true;
          atExplicitKey = true;
          allowCompact = true;
        } else if (atExplicitKey) {
          atExplicitKey = false;
          allowCompact = true;
        } else {
          throwError(state, "incomplete explicit mapping pair; a key node is missed; or followed by a non-tabulated empty line");
        }
        state.position += 1;
        ch = following;
      } else {
        _keyLine = state.line;
        _keyLineStart = state.lineStart;
        _keyPos = state.position;
        if (!composeNode(state, flowIndent, CONTEXT_FLOW_OUT, false, true)) {
          break;
        }
        if (state.line === _line) {
          ch = state.input.charCodeAt(state.position);
          while (isWhiteSpace(ch)) {
            ch = state.input.charCodeAt(++state.position);
          }
          if (ch === 58) {
            ch = state.input.charCodeAt(++state.position);
            if (!isWsOrEol(ch)) {
              throwError(state, "a whitespace character is expected after the key-value separator within a block mapping");
            }
            if (atExplicitKey) {
              storeMappingPair(state, _result, overridableKeys, keyTag, keyNode, null, _keyLine, _keyLineStart, _keyPos);
              keyTag = keyNode = valueNode = null;
            }
            detected = true;
            atExplicitKey = false;
            allowCompact = false;
            keyTag = state.tag;
            keyNode = state.result;
          } else if (detected) {
            throwError(state, "can not read an implicit mapping pair; a colon is missed");
          } else {
            state.tag = _tag;
            state.anchor = _anchor;
            return true;
          }
        } else if (detected) {
          throwError(state, "can not read a block mapping entry; a multiline key may not be an implicit key");
        } else {
          state.tag = _tag;
          state.anchor = _anchor;
          return true;
        }
      }
      if (state.line === _line || state.lineIndent > nodeIndent) {
        if (atExplicitKey) {
          _keyLine = state.line;
          _keyLineStart = state.lineStart;
          _keyPos = state.position;
        }
        if (composeNode(state, nodeIndent, CONTEXT_BLOCK_OUT, true, allowCompact)) {
          if (atExplicitKey) {
            keyNode = state.result;
          } else {
            valueNode = state.result;
          }
        }
        if (!atExplicitKey) {
          storeMappingPair(state, _result, overridableKeys, keyTag, keyNode, valueNode, _keyLine, _keyLineStart, _keyPos);
          keyTag = keyNode = valueNode = null;
        }
        skipSeparationSpace(state, true, -1);
        ch = state.input.charCodeAt(state.position);
      }
      if ((state.line === _line || state.lineIndent > nodeIndent) && ch !== 0) {
        throwError(state, "bad indentation of a mapping entry");
      } else if (state.lineIndent < nodeIndent) {
        break;
      }
    }
    if (atExplicitKey) {
      storeMappingPair(state, _result, overridableKeys, keyTag, keyNode, null, _keyLine, _keyLineStart, _keyPos);
    }
    if (detected) {
      state.tag = _tag;
      state.anchor = _anchor;
      state.kind = "mapping";
      state.result = _result;
    }
    return detected;
  }
  function readTagProperty(state) {
    let isVerbatim = false;
    let isNamed = false;
    let tagHandle;
    let tagName;
    let ch = state.input.charCodeAt(state.position);
    if (ch !== 33) return false;
    if (state.tag !== null) {
      throwError(state, "duplication of a tag property");
    }
    ch = state.input.charCodeAt(++state.position);
    if (ch === 60) {
      isVerbatim = true;
      ch = state.input.charCodeAt(++state.position);
    } else if (ch === 33) {
      isNamed = true;
      tagHandle = "!!";
      ch = state.input.charCodeAt(++state.position);
    } else {
      tagHandle = "!";
    }
    let _position = state.position;
    if (isVerbatim) {
      do {
        ch = state.input.charCodeAt(++state.position);
      } while (ch !== 0 && ch !== 62);
      if (state.position < state.length) {
        tagName = state.input.slice(_position, state.position);
        ch = state.input.charCodeAt(++state.position);
      } else {
        throwError(state, "unexpected end of the stream within a verbatim tag");
      }
    } else {
      while (ch !== 0 && !isWsOrEol(ch)) {
        if (ch === 33) {
          if (!isNamed) {
            tagHandle = state.input.slice(_position - 1, state.position + 1);
            if (!PATTERN_TAG_HANDLE.test(tagHandle)) {
              throwError(state, "named tag handle cannot contain such characters");
            }
            isNamed = true;
            _position = state.position + 1;
          } else {
            throwError(state, "tag suffix cannot contain exclamation marks");
          }
        }
        ch = state.input.charCodeAt(++state.position);
      }
      tagName = state.input.slice(_position, state.position);
      if (PATTERN_FLOW_INDICATORS.test(tagName)) {
        throwError(state, "tag suffix cannot contain flow indicator characters");
      }
    }
    if (tagName && !PATTERN_TAG_URI.test(tagName)) {
      throwError(state, "tag name cannot contain such characters: " + tagName);
    }
    try {
      tagName = decodeURIComponent(tagName);
    } catch (err) {
      throwError(state, "tag name is malformed: " + tagName);
    }
    if (isVerbatim) {
      state.tag = tagName;
    } else if (_hasOwnProperty.call(state.tagMap, tagHandle)) {
      state.tag = state.tagMap[tagHandle] + tagName;
    } else if (tagHandle === "!") {
      state.tag = "!" + tagName;
    } else if (tagHandle === "!!") {
      state.tag = "tag:yaml.org,2002:" + tagName;
    } else {
      throwError(state, 'undeclared tag handle "' + tagHandle + '"');
    }
    return true;
  }
  function readAnchorProperty(state) {
    let ch = state.input.charCodeAt(state.position);
    if (ch !== 38) return false;
    if (state.anchor !== null) {
      throwError(state, "duplication of an anchor property");
    }
    ch = state.input.charCodeAt(++state.position);
    const _position = state.position;
    while (ch !== 0 && !isWsOrEol(ch) && !isFlowIndicator(ch)) {
      ch = state.input.charCodeAt(++state.position);
    }
    if (state.position === _position) {
      throwError(state, "name of an anchor node must contain at least one character");
    }
    state.anchor = state.input.slice(_position, state.position);
    return true;
  }
  function readAlias(state) {
    let ch = state.input.charCodeAt(state.position);
    if (ch !== 42) return false;
    ch = state.input.charCodeAt(++state.position);
    const _position = state.position;
    while (ch !== 0 && !isWsOrEol(ch) && !isFlowIndicator(ch)) {
      ch = state.input.charCodeAt(++state.position);
    }
    if (state.position === _position) {
      throwError(state, "name of an alias node must contain at least one character");
    }
    const alias = state.input.slice(_position, state.position);
    if (!_hasOwnProperty.call(state.anchorMap, alias)) {
      throwError(state, 'unidentified alias "' + alias + '"');
    }
    state.result = state.anchorMap[alias];
    skipSeparationSpace(state, true, -1);
    return true;
  }
  function tryReadBlockMappingFromProperty(state, propertyStart, nodeIndent, flowIndent) {
    const fallbackState = snapshotState(state);
    beginAnchorTransaction(state);
    restoreState(state, propertyStart);
    state.tag = null;
    state.anchor = null;
    state.kind = null;
    state.result = null;
    if (readBlockMapping(state, nodeIndent, flowIndent) && state.kind === "mapping") {
      commitAnchorTransaction(state);
      return true;
    }
    rollbackAnchorTransaction(state);
    restoreState(state, fallbackState);
    return false;
  }
  function composeNode(state, parentIndent, nodeContext, allowToSeek, allowCompact) {
    let allowBlockScalars;
    let allowBlockCollections;
    let indentStatus = 1;
    let atNewLine = false;
    let hasContent = false;
    let propertyStart = null;
    let type2;
    let flowIndent;
    let blockIndent;
    if (state.depth >= state.maxDepth) {
      throwError(state, "nesting exceeded maxDepth (" + state.maxDepth + ")");
    }
    state.depth += 1;
    if (state.listener !== null) {
      state.listener("open", state);
    }
    state.tag = null;
    state.anchor = null;
    state.kind = null;
    state.result = null;
    const allowBlockStyles = allowBlockScalars = allowBlockCollections = CONTEXT_BLOCK_OUT === nodeContext || CONTEXT_BLOCK_IN === nodeContext;
    if (allowToSeek) {
      if (skipSeparationSpace(state, true, -1)) {
        atNewLine = true;
        if (state.lineIndent > parentIndent) {
          indentStatus = 1;
        } else if (state.lineIndent === parentIndent) {
          indentStatus = 0;
        } else if (state.lineIndent < parentIndent) {
          indentStatus = -1;
        }
      }
    }
    if (indentStatus === 1) {
      while (true) {
        const ch = state.input.charCodeAt(state.position);
        const propertyState = snapshotState(state);
        if (atNewLine && (ch === 33 && state.tag !== null || ch === 38 && state.anchor !== null)) {
          break;
        }
        if (!readTagProperty(state) && !readAnchorProperty(state)) {
          break;
        }
        if (propertyStart === null) {
          propertyStart = propertyState;
        }
        if (skipSeparationSpace(state, true, -1)) {
          atNewLine = true;
          allowBlockCollections = allowBlockStyles;
          if (state.lineIndent > parentIndent) {
            indentStatus = 1;
          } else if (state.lineIndent === parentIndent) {
            indentStatus = 0;
          } else if (state.lineIndent < parentIndent) {
            indentStatus = -1;
          }
        } else {
          allowBlockCollections = false;
        }
      }
    }
    if (allowBlockCollections) {
      allowBlockCollections = atNewLine || allowCompact;
    }
    if (indentStatus === 1 || CONTEXT_BLOCK_OUT === nodeContext) {
      if (CONTEXT_FLOW_IN === nodeContext || CONTEXT_FLOW_OUT === nodeContext) {
        flowIndent = parentIndent;
      } else {
        flowIndent = parentIndent + 1;
      }
      blockIndent = state.position - state.lineStart;
      if (indentStatus === 1) {
        if (allowBlockCollections && (readBlockSequence(state, blockIndent) || readBlockMapping(state, blockIndent, flowIndent)) || readFlowCollection(state, flowIndent)) {
          hasContent = true;
        } else {
          const ch = state.input.charCodeAt(state.position);
          if (propertyStart !== null && allowBlockStyles && !allowBlockCollections && ch !== 124 && ch !== 62 && tryReadBlockMappingFromProperty(
            state,
            propertyStart,
            propertyStart.position - propertyStart.lineStart,
            flowIndent
          )) {
            hasContent = true;
          } else if (allowBlockScalars && readBlockScalar(state, flowIndent) || readSingleQuotedScalar(state, flowIndent) || readDoubleQuotedScalar(state, flowIndent)) {
            hasContent = true;
          } else if (readAlias(state)) {
            hasContent = true;
            if (state.tag !== null || state.anchor !== null) {
              throwError(state, "alias node should not have any properties");
            }
          } else if (readPlainScalar(state, flowIndent, CONTEXT_FLOW_IN === nodeContext)) {
            hasContent = true;
            if (state.tag === null) {
              state.tag = "?";
            }
          }
          if (state.anchor !== null) {
            storeAnchor(state, state.anchor, state.result);
          }
        }
      } else if (indentStatus === 0) {
        hasContent = allowBlockCollections && readBlockSequence(state, blockIndent);
      }
    }
    if (state.tag === null) {
      if (state.anchor !== null) {
        storeAnchor(state, state.anchor, state.result);
      }
    } else if (state.tag === "?") {
      if (state.result !== null && state.kind !== "scalar") {
        throwError(state, 'unacceptable node kind for !<?> tag; it should be "scalar", not "' + state.kind + '"');
      }
      for (let typeIndex = 0, typeQuantity = state.implicitTypes.length; typeIndex < typeQuantity; typeIndex += 1) {
        type2 = state.implicitTypes[typeIndex];
        if (type2.resolve(state.result)) {
          state.result = type2.construct(state.result);
          state.tag = type2.tag;
          if (state.anchor !== null) {
            storeAnchor(state, state.anchor, state.result);
          }
          break;
        }
      }
    } else if (state.tag !== "!") {
      if (_hasOwnProperty.call(state.typeMap[state.kind || "fallback"], state.tag)) {
        type2 = state.typeMap[state.kind || "fallback"][state.tag];
      } else {
        type2 = null;
        const typeList = state.typeMap.multi[state.kind || "fallback"];
        for (let typeIndex = 0, typeQuantity = typeList.length; typeIndex < typeQuantity; typeIndex += 1) {
          if (state.tag.slice(0, typeList[typeIndex].tag.length) === typeList[typeIndex].tag) {
            type2 = typeList[typeIndex];
            break;
          }
        }
      }
      if (!type2) {
        throwError(state, "unknown tag !<" + state.tag + ">");
      }
      if (state.result !== null && type2.kind !== state.kind) {
        throwError(state, "unacceptable node kind for !<" + state.tag + '> tag; it should be "' + type2.kind + '", not "' + state.kind + '"');
      }
      if (!type2.resolve(state.result, state.tag)) {
        throwError(state, "cannot resolve a node with !<" + state.tag + "> explicit tag");
      } else {
        state.result = type2.construct(state.result, state.tag);
        if (state.anchor !== null) {
          storeAnchor(state, state.anchor, state.result);
        }
      }
    }
    if (state.listener !== null) {
      state.listener("close", state);
    }
    state.depth -= 1;
    return state.tag !== null || state.anchor !== null || hasContent;
  }
  function readDocument(state) {
    const documentStart = state.position;
    let hasDirectives = false;
    let ch;
    state.version = null;
    state.checkLineBreaks = state.legacy;
    state.tagMap = /* @__PURE__ */ Object.create(null);
    state.anchorMap = /* @__PURE__ */ Object.create(null);
    while ((ch = state.input.charCodeAt(state.position)) !== 0) {
      skipSeparationSpace(state, true, -1);
      ch = state.input.charCodeAt(state.position);
      if (state.lineIndent > 0 || ch !== 37) {
        break;
      }
      hasDirectives = true;
      ch = state.input.charCodeAt(++state.position);
      let _position = state.position;
      while (ch !== 0 && !isWsOrEol(ch)) {
        ch = state.input.charCodeAt(++state.position);
      }
      const directiveName = state.input.slice(_position, state.position);
      const directiveArgs = [];
      if (directiveName.length < 1) {
        throwError(state, "directive name must not be less than one character in length");
      }
      while (ch !== 0) {
        while (isWhiteSpace(ch)) {
          ch = state.input.charCodeAt(++state.position);
        }
        if (ch === 35) {
          do {
            ch = state.input.charCodeAt(++state.position);
          } while (ch !== 0 && !isEol(ch));
          break;
        }
        if (isEol(ch)) break;
        _position = state.position;
        while (ch !== 0 && !isWsOrEol(ch)) {
          ch = state.input.charCodeAt(++state.position);
        }
        directiveArgs.push(state.input.slice(_position, state.position));
      }
      if (ch !== 0) readLineBreak(state);
      if (_hasOwnProperty.call(directiveHandlers, directiveName)) {
        directiveHandlers[directiveName](state, directiveName, directiveArgs);
      } else {
        throwWarning(state, 'unknown document directive "' + directiveName + '"');
      }
    }
    skipSeparationSpace(state, true, -1);
    if (state.lineIndent === 0 && state.input.charCodeAt(state.position) === 45 && state.input.charCodeAt(state.position + 1) === 45 && state.input.charCodeAt(state.position + 2) === 45) {
      state.position += 3;
      skipSeparationSpace(state, true, -1);
    } else if (hasDirectives) {
      throwError(state, "directives end mark is expected");
    }
    composeNode(state, state.lineIndent - 1, CONTEXT_BLOCK_OUT, false, true);
    skipSeparationSpace(state, true, -1);
    if (state.checkLineBreaks && PATTERN_NON_ASCII_LINE_BREAKS.test(state.input.slice(documentStart, state.position))) {
      throwWarning(state, "non-ASCII line breaks are interpreted as content");
    }
    state.documents.push(state.result);
    if (state.position === state.lineStart && testDocumentSeparator(state)) {
      if (state.input.charCodeAt(state.position) === 46) {
        state.position += 3;
        skipSeparationSpace(state, true, -1);
      }
      return;
    }
    if (state.position < state.length - 1) {
      throwError(state, "end of the stream or a document separator is expected");
    }
  }
  function loadDocuments(input, options) {
    input = String(input);
    options = options || {};
    if (input.length !== 0) {
      if (input.charCodeAt(input.length - 1) !== 10 && input.charCodeAt(input.length - 1) !== 13) {
        input += "\n";
      }
      if (input.charCodeAt(0) === 65279) {
        input = input.slice(1);
      }
    }
    const state = new State(input, options);
    const nullpos = input.indexOf("\0");
    if (nullpos !== -1) {
      state.position = nullpos;
      throwError(state, "null byte is not allowed in input");
    }
    state.input += "\0";
    while (state.input.charCodeAt(state.position) === 32) {
      state.lineIndent += 1;
      state.position += 1;
    }
    while (state.position < state.length - 1) {
      readDocument(state);
    }
    return state.documents;
  }
  function loadAll2(input, iterator, options) {
    if (iterator !== null && typeof iterator === "object" && typeof options === "undefined") {
      options = iterator;
      iterator = null;
    }
    const documents = loadDocuments(input, options);
    if (typeof iterator !== "function") {
      return documents;
    }
    for (let index = 0, length = documents.length; index < length; index += 1) {
      iterator(documents[index]);
    }
  }
  function load2(input, options) {
    const documents = loadDocuments(input, options);
    if (documents.length === 0) {
      return void 0;
    } else if (documents.length === 1) {
      return documents[0];
    }
    throw new YAMLException2("expected a single document in the stream, but found more");
  }
  loader.loadAll = loadAll2;
  loader.load = load2;
  return loader;
}
var dumper = {};
var hasRequiredDumper;
function requireDumper() {
  if (hasRequiredDumper) return dumper;
  hasRequiredDumper = 1;
  const common2 = requireCommon();
  const YAMLException2 = requireException();
  const DEFAULT_SCHEMA2 = require_default();
  const _toString = Object.prototype.toString;
  const _hasOwnProperty = Object.prototype.hasOwnProperty;
  const CHAR_BOM = 65279;
  const CHAR_TAB = 9;
  const CHAR_LINE_FEED = 10;
  const CHAR_CARRIAGE_RETURN = 13;
  const CHAR_SPACE = 32;
  const CHAR_EXCLAMATION = 33;
  const CHAR_DOUBLE_QUOTE = 34;
  const CHAR_SHARP = 35;
  const CHAR_PERCENT = 37;
  const CHAR_AMPERSAND = 38;
  const CHAR_SINGLE_QUOTE = 39;
  const CHAR_ASTERISK = 42;
  const CHAR_COMMA = 44;
  const CHAR_MINUS = 45;
  const CHAR_COLON = 58;
  const CHAR_EQUALS = 61;
  const CHAR_GREATER_THAN = 62;
  const CHAR_QUESTION = 63;
  const CHAR_COMMERCIAL_AT = 64;
  const CHAR_LEFT_SQUARE_BRACKET = 91;
  const CHAR_RIGHT_SQUARE_BRACKET = 93;
  const CHAR_GRAVE_ACCENT = 96;
  const CHAR_LEFT_CURLY_BRACKET = 123;
  const CHAR_VERTICAL_LINE = 124;
  const CHAR_RIGHT_CURLY_BRACKET = 125;
  const ESCAPE_SEQUENCES = {};
  ESCAPE_SEQUENCES[0] = "\\0";
  ESCAPE_SEQUENCES[7] = "\\a";
  ESCAPE_SEQUENCES[8] = "\\b";
  ESCAPE_SEQUENCES[9] = "\\t";
  ESCAPE_SEQUENCES[10] = "\\n";
  ESCAPE_SEQUENCES[11] = "\\v";
  ESCAPE_SEQUENCES[12] = "\\f";
  ESCAPE_SEQUENCES[13] = "\\r";
  ESCAPE_SEQUENCES[27] = "\\e";
  ESCAPE_SEQUENCES[34] = '\\"';
  ESCAPE_SEQUENCES[92] = "\\\\";
  ESCAPE_SEQUENCES[133] = "\\N";
  ESCAPE_SEQUENCES[160] = "\\_";
  ESCAPE_SEQUENCES[8232] = "\\L";
  ESCAPE_SEQUENCES[8233] = "\\P";
  const DEPRECATED_BOOLEANS_SYNTAX = [
    "y",
    "Y",
    "yes",
    "Yes",
    "YES",
    "on",
    "On",
    "ON",
    "n",
    "N",
    "no",
    "No",
    "NO",
    "off",
    "Off",
    "OFF"
  ];
  const DEPRECATED_BASE60_SYNTAX = /^[-+]?[0-9_]+(?::[0-9_]+)+(?:\.[0-9_]*)?$/;
  function compileStyleMap(schema2, map2) {
    if (map2 === null) return {};
    const result = {};
    const keys = Object.keys(map2);
    for (let index = 0, length = keys.length; index < length; index += 1) {
      let tag = keys[index];
      let style = String(map2[tag]);
      if (tag.slice(0, 2) === "!!") {
        tag = "tag:yaml.org,2002:" + tag.slice(2);
      }
      const type2 = schema2.compiledTypeMap["fallback"][tag];
      if (type2 && _hasOwnProperty.call(type2.styleAliases, style)) {
        style = type2.styleAliases[style];
      }
      result[tag] = style;
    }
    return result;
  }
  function encodeHex(character) {
    let handle;
    let length;
    const string = character.toString(16).toUpperCase();
    if (character <= 255) {
      handle = "x";
      length = 2;
    } else if (character <= 65535) {
      handle = "u";
      length = 4;
    } else if (character <= 4294967295) {
      handle = "U";
      length = 8;
    } else {
      throw new YAMLException2("code point within a string may not be greater than 0xFFFFFFFF");
    }
    return "\\" + handle + common2.repeat("0", length - string.length) + string;
  }
  const QUOTING_TYPE_SINGLE = 1;
  const QUOTING_TYPE_DOUBLE = 2;
  function State(options) {
    this.schema = options["schema"] || DEFAULT_SCHEMA2;
    this.indent = Math.max(1, options["indent"] || 2);
    this.noArrayIndent = options["noArrayIndent"] || false;
    this.skipInvalid = options["skipInvalid"] || false;
    this.flowLevel = common2.isNothing(options["flowLevel"]) ? -1 : options["flowLevel"];
    this.styleMap = compileStyleMap(this.schema, options["styles"] || null);
    this.sortKeys = options["sortKeys"] || false;
    this.lineWidth = options["lineWidth"] || 80;
    this.noRefs = options["noRefs"] || false;
    this.noCompatMode = options["noCompatMode"] || false;
    this.condenseFlow = options["condenseFlow"] || false;
    this.quotingType = options["quotingType"] === '"' ? QUOTING_TYPE_DOUBLE : QUOTING_TYPE_SINGLE;
    this.forceQuotes = options["forceQuotes"] || false;
    this.replacer = typeof options["replacer"] === "function" ? options["replacer"] : null;
    this.implicitTypes = this.schema.compiledImplicit;
    this.explicitTypes = this.schema.compiledExplicit;
    this.tag = null;
    this.result = "";
    this.duplicates = [];
    this.usedDuplicates = null;
  }
  function indentString(string, spaces) {
    const ind = common2.repeat(" ", spaces);
    let position = 0;
    let result = "";
    const length = string.length;
    while (position < length) {
      let line;
      const next = string.indexOf("\n", position);
      if (next === -1) {
        line = string.slice(position);
        position = length;
      } else {
        line = string.slice(position, next + 1);
        position = next + 1;
      }
      if (line.length && line !== "\n") result += ind;
      result += line;
    }
    return result;
  }
  function generateNextLine(state, level) {
    return "\n" + common2.repeat(" ", state.indent * level);
  }
  function testImplicitResolving(state, str2) {
    for (let index = 0, length = state.implicitTypes.length; index < length; index += 1) {
      const type2 = state.implicitTypes[index];
      if (type2.resolve(str2)) {
        return true;
      }
    }
    return false;
  }
  function isWhitespace(c) {
    return c === CHAR_SPACE || c === CHAR_TAB;
  }
  function isPrintable(c) {
    return c >= 32 && c <= 126 || c >= 161 && c <= 55295 && c !== 8232 && c !== 8233 || c >= 57344 && c <= 65533 && c !== CHAR_BOM || c >= 65536 && c <= 1114111;
  }
  function isNsCharOrWhitespace(c) {
    return isPrintable(c) && c !== CHAR_BOM && // - b-char
    c !== CHAR_CARRIAGE_RETURN && c !== CHAR_LINE_FEED;
  }
  function isPlainSafe(c, prev, inblock) {
    const cIsNsCharOrWhitespace = isNsCharOrWhitespace(c);
    const cIsNsChar = cIsNsCharOrWhitespace && !isWhitespace(c);
    return (
      // ns-plain-safe
      (inblock ? cIsNsCharOrWhitespace : cIsNsCharOrWhitespace && // - c-flow-indicator
      c !== CHAR_COMMA && c !== CHAR_LEFT_SQUARE_BRACKET && c !== CHAR_RIGHT_SQUARE_BRACKET && c !== CHAR_LEFT_CURLY_BRACKET && c !== CHAR_RIGHT_CURLY_BRACKET) && // ns-plain-char
      c !== CHAR_SHARP && // false on '#'
      !(prev === CHAR_COLON && !cIsNsChar) || // false on ': '
      isNsCharOrWhitespace(prev) && !isWhitespace(prev) && c === CHAR_SHARP || // change to true on '[^ ]#'
      prev === CHAR_COLON && cIsNsChar
    );
  }
  function isPlainSafeFirst(c) {
    return isPrintable(c) && c !== CHAR_BOM && !isWhitespace(c) && // - s-white
    // - (c-indicator ::=
    // “-” | “?” | “:” | “,” | “[” | “]” | “{” | “}”
    c !== CHAR_MINUS && c !== CHAR_QUESTION && c !== CHAR_COLON && c !== CHAR_COMMA && c !== CHAR_LEFT_SQUARE_BRACKET && c !== CHAR_RIGHT_SQUARE_BRACKET && c !== CHAR_LEFT_CURLY_BRACKET && c !== CHAR_RIGHT_CURLY_BRACKET && // | “#” | “&” | “*” | “!” | “|” | “=” | “>” | “'” | “"”
    c !== CHAR_SHARP && c !== CHAR_AMPERSAND && c !== CHAR_ASTERISK && c !== CHAR_EXCLAMATION && c !== CHAR_VERTICAL_LINE && c !== CHAR_EQUALS && c !== CHAR_GREATER_THAN && c !== CHAR_SINGLE_QUOTE && c !== CHAR_DOUBLE_QUOTE && // | “%” | “@” | “`”)
    c !== CHAR_PERCENT && c !== CHAR_COMMERCIAL_AT && c !== CHAR_GRAVE_ACCENT;
  }
  function isPlainSafeLast(c) {
    return !isWhitespace(c) && c !== CHAR_COLON;
  }
  function codePointAt(string, pos) {
    const first = string.charCodeAt(pos);
    let second;
    if (first >= 55296 && first <= 56319 && pos + 1 < string.length) {
      second = string.charCodeAt(pos + 1);
      if (second >= 56320 && second <= 57343) {
        return (first - 55296) * 1024 + second - 56320 + 65536;
      }
    }
    return first;
  }
  function needIndentIndicator(string) {
    const leadingSpaceRe = /^\n* /;
    return leadingSpaceRe.test(string);
  }
  const STYLE_PLAIN = 1;
  const STYLE_SINGLE = 2;
  const STYLE_LITERAL = 3;
  const STYLE_FOLDED = 4;
  const STYLE_DOUBLE = 5;
  function chooseScalarStyle(string, singleLineOnly, indentPerLevel, lineWidth, testAmbiguousType, quotingType, forceQuotes, inblock) {
    let i;
    let char = 0;
    let prevChar = null;
    let hasLineBreak = false;
    let hasFoldableLine = false;
    const shouldTrackWidth = lineWidth !== -1;
    let previousLineBreak = -1;
    let plain = isPlainSafeFirst(codePointAt(string, 0)) && isPlainSafeLast(codePointAt(string, string.length - 1));
    if (singleLineOnly || forceQuotes) {
      for (i = 0; i < string.length; char >= 65536 ? i += 2 : i++) {
        char = codePointAt(string, i);
        if (!isPrintable(char)) {
          return STYLE_DOUBLE;
        }
        plain = plain && isPlainSafe(char, prevChar, inblock);
        prevChar = char;
      }
    } else {
      for (i = 0; i < string.length; char >= 65536 ? i += 2 : i++) {
        char = codePointAt(string, i);
        if (char === CHAR_LINE_FEED) {
          hasLineBreak = true;
          if (shouldTrackWidth) {
            hasFoldableLine = hasFoldableLine || // Foldable line = too long, and not more-indented.
            i - previousLineBreak - 1 > lineWidth && string[previousLineBreak + 1] !== " ";
            previousLineBreak = i;
          }
        } else if (!isPrintable(char)) {
          return STYLE_DOUBLE;
        }
        plain = plain && isPlainSafe(char, prevChar, inblock);
        prevChar = char;
      }
      hasFoldableLine = hasFoldableLine || shouldTrackWidth && (i - previousLineBreak - 1 > lineWidth && string[previousLineBreak + 1] !== " ");
    }
    if (!hasLineBreak && !hasFoldableLine) {
      if (plain && !forceQuotes && !testAmbiguousType(string)) {
        return STYLE_PLAIN;
      }
      return quotingType === QUOTING_TYPE_DOUBLE ? STYLE_DOUBLE : STYLE_SINGLE;
    }
    if (indentPerLevel > 9 && needIndentIndicator(string)) {
      return STYLE_DOUBLE;
    }
    if (!forceQuotes) {
      return hasFoldableLine ? STYLE_FOLDED : STYLE_LITERAL;
    }
    return quotingType === QUOTING_TYPE_DOUBLE ? STYLE_DOUBLE : STYLE_SINGLE;
  }
  function writeScalar(state, string, level, iskey, inblock) {
    state.dump = (function() {
      if (string.length === 0) {
        return state.quotingType === QUOTING_TYPE_DOUBLE ? '""' : "''";
      }
      if (!state.noCompatMode) {
        if (DEPRECATED_BOOLEANS_SYNTAX.indexOf(string) !== -1 || DEPRECATED_BASE60_SYNTAX.test(string)) {
          return state.quotingType === QUOTING_TYPE_DOUBLE ? '"' + string + '"' : "'" + string + "'";
        }
      }
      const indent = state.indent * Math.max(1, level);
      const lineWidth = state.lineWidth === -1 ? -1 : Math.max(Math.min(state.lineWidth, 40), state.lineWidth - indent);
      const singleLineOnly = iskey || // No block styles in flow mode.
      state.flowLevel > -1 && level >= state.flowLevel;
      function testAmbiguity(string2) {
        return testImplicitResolving(state, string2);
      }
      switch (chooseScalarStyle(
        string,
        singleLineOnly,
        state.indent,
        lineWidth,
        testAmbiguity,
        state.quotingType,
        state.forceQuotes && !iskey,
        inblock
      )) {
        case STYLE_PLAIN:
          return string;
        case STYLE_SINGLE:
          return "'" + string.replace(/'/g, "''") + "'";
        case STYLE_LITERAL:
          return "|" + blockHeader(string, state.indent) + dropEndingNewline(indentString(string, indent));
        case STYLE_FOLDED:
          return ">" + blockHeader(string, state.indent) + dropEndingNewline(indentString(foldString(string, lineWidth), indent));
        case STYLE_DOUBLE:
          return '"' + escapeString(string) + '"';
        default:
          throw new YAMLException2("impossible error: invalid scalar style");
      }
    })();
  }
  function blockHeader(string, indentPerLevel) {
    const indentIndicator = needIndentIndicator(string) ? String(indentPerLevel) : "";
    const clip = string[string.length - 1] === "\n";
    const keep = clip && (string[string.length - 2] === "\n" || string === "\n");
    const chomp = keep ? "+" : clip ? "" : "-";
    return indentIndicator + chomp + "\n";
  }
  function dropEndingNewline(string) {
    return string[string.length - 1] === "\n" ? string.slice(0, -1) : string;
  }
  function foldString(string, width) {
    const lineRe = /(\n+)([^\n]*)/g;
    let result = (function() {
      let nextLF = string.indexOf("\n");
      nextLF = nextLF !== -1 ? nextLF : string.length;
      lineRe.lastIndex = nextLF;
      return foldLine(string.slice(0, nextLF), width);
    })();
    let prevMoreIndented = string[0] === "\n" || string[0] === " ";
    let moreIndented;
    let match;
    while (match = lineRe.exec(string)) {
      const prefix = match[1];
      const line = match[2];
      moreIndented = line[0] === " ";
      result += prefix + (!prevMoreIndented && !moreIndented && line !== "" ? "\n" : "") + foldLine(line, width);
      prevMoreIndented = moreIndented;
    }
    return result;
  }
  function foldLine(line, width) {
    if (line === "" || line[0] === " ") return line;
    const breakRe = / [^ ]/g;
    let match;
    let start = 0;
    let end;
    let curr = 0;
    let next = 0;
    let result = "";
    while (match = breakRe.exec(line)) {
      next = match.index;
      if (next - start > width) {
        end = curr > start ? curr : next;
        result += "\n" + line.slice(start, end);
        start = end + 1;
      }
      curr = next;
    }
    result += "\n";
    if (line.length - start > width && curr > start) {
      result += line.slice(start, curr) + "\n" + line.slice(curr + 1);
    } else {
      result += line.slice(start);
    }
    return result.slice(1);
  }
  function escapeString(string) {
    let result = "";
    let char = 0;
    for (let i = 0; i < string.length; char >= 65536 ? i += 2 : i++) {
      char = codePointAt(string, i);
      const escapeSeq = ESCAPE_SEQUENCES[char];
      if (!escapeSeq && isPrintable(char)) {
        result += string[i];
        if (char >= 65536) result += string[i + 1];
      } else {
        result += escapeSeq || encodeHex(char);
      }
    }
    return result;
  }
  function writeFlowSequence(state, level, object) {
    let _result = "";
    const _tag = state.tag;
    for (let index = 0, length = object.length; index < length; index += 1) {
      let value = object[index];
      if (state.replacer) {
        value = state.replacer.call(object, String(index), value);
      }
      if (writeNode(state, level, value, false, false) || typeof value === "undefined" && writeNode(state, level, null, false, false)) {
        if (_result !== "") _result += "," + (!state.condenseFlow ? " " : "");
        _result += state.dump;
      }
    }
    state.tag = _tag;
    state.dump = "[" + _result + "]";
  }
  function writeBlockSequence(state, level, object, compact) {
    let _result = "";
    const _tag = state.tag;
    for (let index = 0, length = object.length; index < length; index += 1) {
      let value = object[index];
      if (state.replacer) {
        value = state.replacer.call(object, String(index), value);
      }
      if (writeNode(state, level + 1, value, true, true, false, true) || typeof value === "undefined" && writeNode(state, level + 1, null, true, true, false, true)) {
        if (!compact || _result !== "") {
          _result += generateNextLine(state, level);
        }
        if (state.dump && CHAR_LINE_FEED === state.dump.charCodeAt(0)) {
          _result += "-";
        } else {
          _result += "- ";
        }
        _result += state.dump;
      }
    }
    state.tag = _tag;
    state.dump = _result || "[]";
  }
  function writeFlowMapping(state, level, object) {
    let _result = "";
    const _tag = state.tag;
    const objectKeyList = Object.keys(object);
    for (let index = 0, length = objectKeyList.length; index < length; index += 1) {
      let pairBuffer = "";
      if (_result !== "") pairBuffer += ", ";
      if (state.condenseFlow) pairBuffer += '"';
      const objectKey = objectKeyList[index];
      let objectValue = object[objectKey];
      if (state.replacer) {
        objectValue = state.replacer.call(object, objectKey, objectValue);
      }
      if (!writeNode(state, level, objectKey, false, false)) {
        continue;
      }
      if (state.dump.length > 1024) pairBuffer += "? ";
      pairBuffer += state.dump + (state.condenseFlow ? '"' : "") + ":" + (state.condenseFlow ? "" : " ");
      if (!writeNode(state, level, objectValue, false, false)) {
        continue;
      }
      pairBuffer += state.dump;
      _result += pairBuffer;
    }
    state.tag = _tag;
    state.dump = "{" + _result + "}";
  }
  function writeBlockMapping(state, level, object, compact) {
    let _result = "";
    const _tag = state.tag;
    const objectKeyList = Object.keys(object);
    if (state.sortKeys === true) {
      objectKeyList.sort();
    } else if (typeof state.sortKeys === "function") {
      objectKeyList.sort(state.sortKeys);
    } else if (state.sortKeys) {
      throw new YAMLException2("sortKeys must be a boolean or a function");
    }
    for (let index = 0, length = objectKeyList.length; index < length; index += 1) {
      let pairBuffer = "";
      if (!compact || _result !== "") {
        pairBuffer += generateNextLine(state, level);
      }
      const objectKey = objectKeyList[index];
      let objectValue = object[objectKey];
      if (state.replacer) {
        objectValue = state.replacer.call(object, objectKey, objectValue);
      }
      if (!writeNode(state, level + 1, objectKey, true, true, true)) {
        continue;
      }
      const explicitPair = state.tag !== null && state.tag !== "?" || state.dump && state.dump.length > 1024;
      if (explicitPair) {
        if (state.dump && CHAR_LINE_FEED === state.dump.charCodeAt(0)) {
          pairBuffer += "?";
        } else {
          pairBuffer += "? ";
        }
      }
      pairBuffer += state.dump;
      if (explicitPair) {
        pairBuffer += generateNextLine(state, level);
      }
      if (!writeNode(state, level + 1, objectValue, true, explicitPair)) {
        continue;
      }
      if (state.dump && CHAR_LINE_FEED === state.dump.charCodeAt(0)) {
        pairBuffer += ":";
      } else {
        pairBuffer += ": ";
      }
      pairBuffer += state.dump;
      _result += pairBuffer;
    }
    state.tag = _tag;
    state.dump = _result || "{}";
  }
  function detectType(state, object, explicit) {
    const typeList = explicit ? state.explicitTypes : state.implicitTypes;
    for (let index = 0, length = typeList.length; index < length; index += 1) {
      const type2 = typeList[index];
      if ((type2.instanceOf || type2.predicate) && (!type2.instanceOf || typeof object === "object" && object instanceof type2.instanceOf) && (!type2.predicate || type2.predicate(object))) {
        if (explicit) {
          if (type2.multi && type2.representName) {
            state.tag = type2.representName(object);
          } else {
            state.tag = type2.tag;
          }
        } else {
          state.tag = "?";
        }
        if (type2.represent) {
          const style = state.styleMap[type2.tag] || type2.defaultStyle;
          let _result;
          if (_toString.call(type2.represent) === "[object Function]") {
            _result = type2.represent(object, style);
          } else if (_hasOwnProperty.call(type2.represent, style)) {
            _result = type2.represent[style](object, style);
          } else {
            throw new YAMLException2("!<" + type2.tag + '> tag resolver accepts not "' + style + '" style');
          }
          state.dump = _result;
        }
        return true;
      }
    }
    return false;
  }
  function writeNode(state, level, object, block, compact, iskey, isblockseq) {
    state.tag = null;
    state.dump = object;
    if (!detectType(state, object, false)) {
      detectType(state, object, true);
    }
    const type2 = _toString.call(state.dump);
    const inblock = block;
    if (block) {
      block = state.flowLevel < 0 || state.flowLevel > level;
    }
    const objectOrArray = type2 === "[object Object]" || type2 === "[object Array]";
    let duplicateIndex;
    let duplicate;
    if (objectOrArray) {
      duplicateIndex = state.duplicates.indexOf(object);
      duplicate = duplicateIndex !== -1;
    }
    if (state.tag !== null && state.tag !== "?" || duplicate || state.indent !== 2 && level > 0) {
      compact = false;
    }
    if (duplicate && state.usedDuplicates[duplicateIndex]) {
      state.dump = "*ref_" + duplicateIndex;
    } else {
      if (objectOrArray && duplicate && !state.usedDuplicates[duplicateIndex]) {
        state.usedDuplicates[duplicateIndex] = true;
      }
      if (type2 === "[object Object]") {
        if (block && Object.keys(state.dump).length !== 0) {
          writeBlockMapping(state, level, state.dump, compact);
          if (duplicate) {
            state.dump = "&ref_" + duplicateIndex + state.dump;
          }
        } else {
          writeFlowMapping(state, level, state.dump);
          if (duplicate) {
            state.dump = "&ref_" + duplicateIndex + " " + state.dump;
          }
        }
      } else if (type2 === "[object Array]") {
        if (block && state.dump.length !== 0) {
          if (state.noArrayIndent && !isblockseq && level > 0) {
            writeBlockSequence(state, level - 1, state.dump, compact);
          } else {
            writeBlockSequence(state, level, state.dump, compact);
          }
          if (duplicate) {
            state.dump = "&ref_" + duplicateIndex + state.dump;
          }
        } else {
          writeFlowSequence(state, level, state.dump);
          if (duplicate) {
            state.dump = "&ref_" + duplicateIndex + " " + state.dump;
          }
        }
      } else if (type2 === "[object String]") {
        if (state.tag !== "?") {
          writeScalar(state, state.dump, level, iskey, inblock);
        }
      } else if (type2 === "[object Undefined]") {
        return false;
      } else {
        if (state.skipInvalid) return false;
        throw new YAMLException2("unacceptable kind of an object to dump " + type2);
      }
      if (state.tag !== null && state.tag !== "?") {
        let tagStr = encodeURI(
          state.tag[0] === "!" ? state.tag.slice(1) : state.tag
        ).replace(/!/g, "%21");
        if (state.tag[0] === "!") {
          tagStr = "!" + tagStr;
        } else if (tagStr.slice(0, 18) === "tag:yaml.org,2002:") {
          tagStr = "!!" + tagStr.slice(18);
        } else {
          tagStr = "!<" + tagStr + ">";
        }
        state.dump = tagStr + " " + state.dump;
      }
    }
    return true;
  }
  function getDuplicateReferences(object, state) {
    const objects = [];
    const duplicatesIndexes = [];
    inspectNode(object, objects, duplicatesIndexes);
    const length = duplicatesIndexes.length;
    for (let index = 0; index < length; index += 1) {
      state.duplicates.push(objects[duplicatesIndexes[index]]);
    }
    state.usedDuplicates = new Array(length);
  }
  function inspectNode(object, objects, duplicatesIndexes) {
    if (object !== null && typeof object === "object") {
      const index = objects.indexOf(object);
      if (index !== -1) {
        if (duplicatesIndexes.indexOf(index) === -1) {
          duplicatesIndexes.push(index);
        }
      } else {
        objects.push(object);
        if (Array.isArray(object)) {
          for (let i = 0, length = object.length; i < length; i += 1) {
            inspectNode(object[i], objects, duplicatesIndexes);
          }
        } else {
          const objectKeyList = Object.keys(object);
          for (let i = 0, length = objectKeyList.length; i < length; i += 1) {
            inspectNode(object[objectKeyList[i]], objects, duplicatesIndexes);
          }
        }
      }
    }
  }
  function dump2(input, options) {
    options = options || {};
    const state = new State(options);
    if (!state.noRefs) getDuplicateReferences(input, state);
    let value = input;
    if (state.replacer) {
      value = state.replacer.call({ "": value }, "", value);
    }
    if (writeNode(state, 0, value, true, true)) return state.dump + "\n";
    return "";
  }
  dumper.dump = dump2;
  return dumper;
}
var hasRequiredJsYaml;
function requireJsYaml() {
  if (hasRequiredJsYaml) return jsYaml;
  hasRequiredJsYaml = 1;
  const loader2 = requireLoader();
  const dumper2 = requireDumper();
  function renamed(from, to) {
    return function() {
      throw new Error("Function yaml." + from + " is removed in js-yaml 4. Use yaml." + to + " instead, which is now safe by default.");
    };
  }
  jsYaml.Type = requireType();
  jsYaml.Schema = requireSchema();
  jsYaml.FAILSAFE_SCHEMA = requireFailsafe();
  jsYaml.JSON_SCHEMA = requireJson();
  jsYaml.CORE_SCHEMA = requireCore();
  jsYaml.DEFAULT_SCHEMA = require_default();
  jsYaml.load = loader2.load;
  jsYaml.loadAll = loader2.loadAll;
  jsYaml.dump = dumper2.dump;
  jsYaml.YAMLException = requireException();
  jsYaml.types = {
    binary: requireBinary(),
    float: requireFloat(),
    map: requireMap(),
    null: require_null(),
    pairs: requirePairs(),
    set: requireSet(),
    timestamp: requireTimestamp(),
    bool: requireBool(),
    int: requireInt(),
    merge: requireMerge(),
    omap: requireOmap(),
    seq: requireSeq(),
    str: requireStr()
  };
  jsYaml.safeLoad = renamed("safeLoad", "load");
  jsYaml.safeLoadAll = renamed("safeLoadAll", "loadAll");
  jsYaml.safeDump = renamed("safeDump", "dump");
  return jsYaml;
}
var jsYamlExports = requireJsYaml();
var yaml = /* @__PURE__ */ getDefaultExportFromCjs(jsYamlExports);
var {
  Type,
  Schema,
  FAILSAFE_SCHEMA,
  JSON_SCHEMA,
  CORE_SCHEMA,
  DEFAULT_SCHEMA,
  load,
  loadAll,
  dump,
  YAMLException,
  types,
  safeLoad,
  safeLoadAll,
  safeDump
} = yaml;

// scripts/v2-diagnostics.mjs
var asArray = (value) => Array.isArray(value) ? value : [];
var isV2 = (document) => document?.schema_version === "project-os-schema/v2";
var isV21 = (document) => document?.schema_version === "project-os-schema/v2.1";
var isV22 = (document) => document?.schema_version === "project-os-schema/v2.2";
var isC4 = (document) => isV2(document) || isV21(document) || isV22(document);
var V22_VERSION = "project-os-schema/v2.2";
function v2Diagnostics(document) {
  if (!isC4(document)) return { errors: [], warnings: [] };
  const nodes = asArray(document.nodes), edges = asArray(document.edges);
  const context = {
    nodes: new Map(nodes.map((node) => [node?.id, node])),
    graphByView: new Map(asArray(document.graphs).map((graph) => [graph?.view, graph?.id])),
    edges,
    v21: isV21(document) || isV22(document),
    v22: isV22(document)
  };
  const errors = [];
  validateContainment(nodes, context, errors);
  validatePortals(nodes, context, errors);
  validateEdges(edges, context, errors);
  validateFlows(asArray(document.flows), context, errors);
  if (context.v21) validateV21(document, context, errors);
  if (isC4(document)) validateV22Rules(document, context, errors);
  const diagnostics = { errors, warnings: budgetWarnings(nodes) };
  if (context.v21) diagnostics.clustering = clusteringHonesty(nodes, edges);
  return diagnostics;
}
function validateContainment(nodes, { nodes: nodeById }, errors) {
  const cycles = /* @__PURE__ */ new Set();
  for (const node of nodes) {
    if (typeof node?.intent === "string" && node.intent.length > 60) errors.push(`${node.id}: intent must be at most 60 characters`);
    if (!node?.parent) continue;
    const parent = nodeById.get(node.parent);
    if (!parent || parent.graph !== node.graph) errors.push(`${node.id}: parent must reference an existing node in the same graph`);
  }
  for (const node of nodes) validateDepth(node, nodeById, cycles, errors);
}
function validateDepth(node, nodeById, cycles, errors) {
  const path2 = [], seen = /* @__PURE__ */ new Set();
  let current = node;
  while (current?.parent) {
    const parent = nodeById.get(current.parent);
    if (!parent || parent.graph !== current.graph) return;
    if (seen.has(parent.id)) {
      const key = cycleKey(parent, nodeById);
      if (!cycles.has(key)) errors.push(`${node.id}: containment cycle`);
      cycles.add(key);
      return;
    }
    seen.add(current.id);
    path2.push(current.id);
    current = parent;
  }
  if (path2.length + 1 > 4) errors.push(`${node.id}: containment depth exceeds 4`);
}
function cycleKey(node, nodes) {
  const ids = [];
  for (let current = node; current && !ids.includes(current.id); current = nodes.get(current.parent)) ids.push(current.id);
  return ids.sort().join("/");
}
function validatePortals(nodes, { graphByView, nodes: nodeById, v21 }, errors) {
  for (const node of nodes) for (const portal of asArray(node?.portals)) {
    const target = nodeById.get(portal?.node);
    if (!target) errors.push(`${node.id}: portal target must exist in view ${portal?.view}`);
    else if (v21 && isSuperseded(target)) errors.push(`${node.id}: portal target ${target.id} references superseded node; use ${target.superseded_by}`);
    else if (target.graph !== graphByView.get(portal?.view)) errors.push(`${node.id}: portal target must exist in view ${portal?.view}`);
    else if (target.graph === node.graph) errors.push(`${node.id}: portal target must be in a different view`);
  }
}
function validateEdges(edges, context, errors) {
  const pairs2 = /* @__PURE__ */ new Set();
  for (const edge of edges) {
    const source = edge?.source ?? edge?.from, target = edge?.target ?? edge?.to;
    const graph = edge?.graph;
    const both = edge?.direction === "both";
    for (const key of both ? [pairKey(graph, source, target), pairKey(graph, target, source)] : [pairKey(graph, source, target)]) {
      if (pairs2.has(key)) errors.push(`${edge?.id}: duplicate directed edge ${key}`);
      pairs2.add(key);
    }
    if (context.v21) {
      validateSupersededEndpoint(source, "source", edge?.id, context.nodes, errors);
      validateSupersededEndpoint(target, "target", edge?.id, context.nodes, errors);
    }
    validateRelations(edge, context.nodes, errors, context.v21);
  }
}
function pairKey(graph, source, target) {
  return `${graph}/${source}/${target}`;
}
function validateV22Rules(document, { nodes: nodeById, v22 }, errors) {
  const nodes = asArray(document.nodes), edges = asArray(document.edges);
  for (const node of nodes) {
    if (node?.type === "router" && !v22) errors.push(`${node.id}: router nodes require ${V22_VERSION}`);
  }
  for (const edge of edges) {
    if (edge?.direction !== void 0) {
      if (!v22) errors.push(`${edge?.id}: edge direction requires ${V22_VERSION}`);
      if (edge.direction !== "both") errors.push(`${edge?.id}: direction must be both`);
    }
    const source = nodeById.get(edge?.source ?? edge?.from);
    for (const relation of asArray(edge?.relations)) {
      if (relation?.condition === void 0) continue;
      const prefix = `${edge?.id}: relation ${relation?.id}`;
      if (!v22) errors.push(`${prefix} condition requires ${V22_VERSION}`);
      if (typeof relation.condition !== "string" || !relation.condition.trim()) errors.push(`${prefix} condition must be a non-empty string`);
      if (source?.type !== "router") errors.push(`${prefix} condition requires a router source`);
    }
  }
  if (v22) for (const node of nodes) {
    if (node?.type !== "router") continue;
    const conditioned = edges.filter((edge) => (edge?.source ?? edge?.from) === node.id).flatMap((edge) => asArray(edge?.relations)).filter((relation) => typeof relation?.condition === "string" && relation.condition.trim()).length;
    if (conditioned < 2) errors.push(`${node.id}: router requires at least two conditioned outbound relations`);
  }
}
function validateRelations(edge, nodes, errors, v21) {
  const source = edge?.source ?? edge?.from, target = edge?.target ?? edge?.to;
  for (const relation of asArray(edge?.relations)) {
    const prefix = `${edge?.id}: relation ${relation?.id}`;
    if (!(/* @__PURE__ */ new Set(["sync", "async", "batch"])).has(relation?.mechanism)) errors.push(`${prefix} mechanism must be sync, async, or batch`);
    validateRelationChild(relation?.source_child, source, "source_child", prefix, nodes, errors, v21);
    validateRelationChild(relation?.target_child, target, "target_child", prefix, nodes, errors, v21);
  }
}
function validateRelationChild(child, parent, field, prefix, nodes, errors, v21) {
  const target = nodes.get(child);
  if (v21 && isSuperseded(target)) return errors.push(`${prefix} ${field} ${target.id} references superseded node; use ${target.superseded_by}`);
  if (child !== void 0 && !isDescendant(child, parent, nodes)) errors.push(`${prefix} ${field} must be a descendant of ${parent}`);
}
function isDescendant(child, parent, nodes) {
  const seen = /* @__PURE__ */ new Set();
  for (let node = nodes.get(child); node?.parent && !seen.has(node.id); node = nodes.get(node.parent)) {
    if (node.parent === parent) return true;
    seen.add(node.id);
  }
  return false;
}
function validateFlows(flows, { graphByView, nodes, edges, v21 }, errors) {
  const relations = new Set(edges.flatMap((edge) => asArray(edge?.relations).map((relation) => relation?.id)));
  for (const flow of flows) {
    const graph = graphByView.get(flow?.view);
    if (!graph) errors.push(`${flow?.id}: view must reference an existing graph`);
    const participants = new Set(asArray(flow?.participants));
    for (const participant of participants) {
      const node = nodes.get(participant);
      if (!node) errors.push(`${flow?.id}: participant ${participant} must exist in flow view graph`);
      else if (v21 && isSuperseded(node)) errors.push(`${flow?.id}: participant ${participant} references superseded node; use ${node.superseded_by}`);
      else if (node.graph !== graph) errors.push(`${flow?.id}: participant ${participant} must exist in flow view graph`);
      else if ([...nodes.values()].some((candidate) => candidate?.parent === participant)) errors.push(`${flow?.id}: participant ${participant} must be a leaf node`);
    }
    for (const step of asArray(flow?.steps)) validateStep(step, flow?.id, participants, relations, errors);
  }
}
function validateStep(step, flowId, participants, relations, errors) {
  for (const endpoint of ["from", "to"]) if (!participants.has(step?.[endpoint])) errors.push(`${flowId}: step ${endpoint} ${step?.[endpoint]} must be a participant`);
  if (step?.relation !== void 0 && !relations.has(step.relation)) errors.push(`${flowId}: step relation ${step.relation} must reference an existing edge relation`);
}
function budgetWarnings(nodes) {
  const counts = /* @__PURE__ */ new Map();
  for (const node of nodes) if (node?.parent) counts.set(node.parent, (counts.get(node.parent) ?? 0) + 1);
  return [...counts].flatMap(([parent, count]) => count > 25 ? [`${parent}: ${count} direct children split/group strongly required (LOD authoring budget \xA73.1)`] : count > 20 ? [`${parent}: ${count} direct children over authoring ceiling \u2014 introduce a grouping level (LOD authoring budget \xA73.1)`] : count > 15 ? [`${parent}: ${count} direct children over soft budget \u2014 consider grouping (LOD authoring budget \xA73.1)`] : []);
}
function validateV21(document, { nodes: nodeById }, errors) {
  for (const node of asArray(document.nodes)) {
    if (node?.status === "erroneous" && !hasContradictionEvidence(node?.reality?.evidence)) errors.push(`${node.id}: erroneous status requires reality.evidence`);
    if (node?.blocking !== void 0 && typeof node.blocking !== "boolean") errors.push(`${node.id}: blocking must be a boolean`);
    else if (node?.blocking !== void 0 && node.status !== "open-question") errors.push(`${node.id}: blocking is only valid for open-question nodes`);
    if (node?.entity !== void 0 && typeof node.entity !== "string") errors.push(`${node.id}: entity must be a string`);
    validateTombstone(node, nodeById, errors);
  }
  validateGate(document.gate, errors);
}
function validateTombstone(node, nodes, errors) {
  if (!isSuperseded(node)) return;
  if (Object.keys(node).some((key) => !["id", "graph", "superseded_by"].includes(key))) errors.push(`${node.id}: superseded tombstone may only contain id, graph, superseded_by`);
  const replacement = nodes.get(node.superseded_by);
  if (!replacement) errors.push(`${node.id}: superseded_by must reference an existing node`);
  else if (isSuperseded(replacement)) errors.push(`${node.id}: superseded_by must not reference a superseded node`);
}
function validateGate(gate, errors) {
  if (gate === void 0) return;
  if (!gate || typeof gate !== "object" || Array.isArray(gate)) return errors.push("gate must be an object");
  if (!isIsoDate(gate.triaged)) errors.push("gate.triaged must be an ISO date");
  if (typeof gate.triaged_by !== "string" || !gate.triaged_by) errors.push("gate.triaged_by must be a string");
}
function validateSupersededEndpoint(id, field, edgeId, nodes, errors) {
  const node = nodes.get(id);
  if (isSuperseded(node)) errors.push(`${edgeId}: ${field} ${id} references superseded node; use ${node.superseded_by}`);
}
function isSuperseded(node) {
  return typeof node?.superseded_by === "string" && node.superseded_by;
}
function hasContradictionEvidence(evidence) {
  return asArray(evidence).some((item) => typeof item === "string" && item.trim());
}
function isIsoDate(value) {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const parsed = /* @__PURE__ */ new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(parsed.valueOf()) && parsed.toISOString().slice(0, 10) === value;
}
function clusteringHonesty(nodes, edges) {
  const children = /* @__PURE__ */ new Map();
  for (const node of nodes) if (node?.parent) children.set(node.parent, [...children.get(node.parent) ?? [], node.id]);
  return [...children].filter(([, ids]) => ids.length >= 2).sort(([left], [right]) => left.localeCompare(right)).map(([parent, ids]) => {
    const direct = new Set(ids);
    let internal = 0, cross = 0;
    for (const edge of edges) {
      const source = edge?.source ?? edge?.from, target = edge?.target ?? edge?.to;
      if (direct.has(source) && direct.has(target)) internal += 1;
      else if (direct.has(source) || direct.has(target)) cross += 1;
    }
    const total = internal + cross;
    return `${parent} internal=${internal}/${total} (${share(internal, total)}) cross=${cross}/${total} (${share(cross, total)})`;
  });
}
function share(part, total) {
  return total ? `${(part / total * 100).toFixed(1)}%` : "n/a";
}

// scripts/export-errors.mjs
function exportErrors(document) {
  const errors = [];
  if (!document || typeof document !== "object" || Array.isArray(document)) return ["project document must be an object"];
  if (!["project-os-schema/v1", "project-os-schema/v2", "project-os-schema/v2.1"].includes(document.schema_version)) errors.push("schema_version must be project-os-schema/v1");
  if (!document.project || typeof document.project.label !== "string" || !document.project.label) errors.push("project.label is required");
  for (const field of ["views", "graphs", "nodes", "edges"]) if (!Array.isArray(document[field])) errors.push(`${field} must be an array`);
  if (Array.isArray(document.views)) document.views.forEach((view, index) => scalarErrors(view, ["id", "label", "type", "memo"], `views/${index}`, errors));
  if (Array.isArray(document.graphs)) document.graphs.forEach((graph, index) => scalarErrors(graph, ["id"], `graphs/${index}`, errors));
  if (Array.isArray(document.nodes)) document.nodes.forEach((node, index) => scalarErrors(node, ["id", "graph", "label", "type", "summary"], `nodes/${index}`, errors));
  if (Array.isArray(document.edges)) document.edges.forEach((edge, index) => scalarErrors(edge, ["id", "source", "target", "type"], `edges/${index}`, errors));
  if (Array.isArray(document.graphs) && Array.isArray(document.nodes)) graphErrors(document, errors);
  if (["project-os-schema/v2", "project-os-schema/v2.1"].includes(document.schema_version)) v2RendererErrors(document, errors);
  errors.push(...v2Diagnostics(document).errors);
  return errors;
}
function graphErrors(document, errors) {
  const graphIds = new Set(document.graphs.map((graph) => graph?.id));
  document.nodes.forEach((node, index) => {
    if (node?.graph && !graphIds.has(node.graph)) errors.push(`nodes/${index}.graph must reference an existing graph`);
  });
}
function scalarErrors(value, fields, prefix, errors) {
  for (const field of fields) if (!value || typeof value[field] !== "string" || !value[field]) errors.push(`${prefix}.${field} is required`);
}
function v2RendererErrors(document, errors) {
  optionalArrayError(document, "flows", "flows", validFlow, errors);
  for (const [index, node] of (Array.isArray(document.nodes) ? document.nodes : []).entries()) optionalArrayError(node, "portals", `nodes/${index}.portals`, (portal) => strings(portal, ["view", "node"]), errors);
  for (const [index, edge] of (Array.isArray(document.edges) ? document.edges : []).entries()) optionalArrayError(edge, "relations", `edges/${index}.relations`, validRelation, errors);
}
function optionalArrayError(value, key, path2, valid, errors) {
  if (value?.[key] !== void 0 && (!Array.isArray(value[key]) || !value[key].every(valid))) errors.push(`${path2} must be an array of valid renderer values`);
}
function validFlow(flow) {
  return strings(flow, ["id", "view", "label"]) && stringArray(flow.participants) && Array.isArray(flow.steps) && flow.steps.every((step) => strings(step, ["from", "to", "message"]) && optionalString(step, "payload") && optionalString(step, "relation"));
}
function validRelation(relation) {
  return strings(relation, ["id", "label", "description", "mechanism"]) && optionalString(relation, "source_child") && optionalString(relation, "target_child") && (relation.payloads === void 0 || stringArray(relation.payloads));
}
function strings(value, keys) {
  return !!value && typeof value === "object" && keys.every((key) => typeof value[key] === "string" && value[key]);
}
function optionalString(value, key) {
  return value?.[key] === void 0 || typeof value[key] === "string";
}
function stringArray(value) {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

// scripts/export-portable.mjs
var markerStart = '<script id="project-os-data" type="application/json">';
var markerEnd = "</script>";
function safeJson(value) {
  return value.replace(/[<>&\u2028\u2029]/g, (character) => ({ "<": "\\u003C", ">": "\\u003E", "&": "\\u0026", "\u2028": "\\u2028", "\u2029": "\\u2029" })[character]);
}
function markerContent(html) {
  return projectDataMarker(html).content;
}
function injectProjectData(template, serialized) {
  verifyTemplate(template);
  const marker = projectDataMarker(template);
  if (marker.content !== "{}") throw Error("Export template marker must be empty");
  return template.slice(0, marker.start) + markerStart + safeJson(serialized) + markerEnd + template.slice(marker.end + 1);
}
function tagEnd(html, start) {
  let quote = "";
  for (let end = start + 2; end < html.length; end += 1) {
    if (quote && html[end] === quote) quote = "";
    else if (!quote && /['"]/.test(html[end])) quote = html[end];
    else if (!quote && html[end] === ">") return end;
  }
  throw Error("Export template contains an unterminated tag");
}
function nextTag(html, from) {
  for (let start = html.indexOf("<", from); start >= 0; ) {
    if (html.startsWith("<!--", start)) {
      const end2 = html.indexOf("-->", start + 4);
      start = end2 < 0 ? -1 : html.indexOf("<", end2 + 3);
      continue;
    }
    if (!/[a-z/]/i.test(html[start + 1] ?? "")) {
      start = html.indexOf("<", start + 1);
      continue;
    }
    const end = tagEnd(html, start);
    const shell = html.slice(start, end + 1);
    const match = /^<\/?([a-z][\w:-]*)\b/i.exec(shell);
    if (match) return { start, end, name: match[1].toLowerCase(), closing: shell[1] === "/", shell };
    start = html.indexOf("<", start + 1);
  }
  return null;
}
function rawTextEnd(html, from, name) {
  const needle = `</${name}`;
  const source = html.toLowerCase();
  for (let start = source.indexOf(needle, from); start >= 0; start = source.indexOf(needle, start + 1)) {
    if (/\s|>/.test(html[start + needle.length] ?? "")) return tagEnd(html, start);
  }
  throw Error(`Export template contains an unterminated ${name} tag`);
}
function attributes(shell) {
  const tag = /^<\/?[a-z][\w:-]*\b/i.exec(shell);
  const result = [];
  for (let cursor = tag?.[0].length ?? shell.length; cursor < shell.length; ) {
    while (/[\s/]/.test(shell[cursor] ?? "")) cursor += 1;
    if (shell[cursor] === ">" || cursor >= shell.length) break;
    const start = cursor;
    while (!/[\s=/>]/.test(shell[cursor] ?? ">")) cursor += 1;
    const name = shell.slice(start, cursor).toLowerCase();
    while (/\s/.test(shell[cursor] ?? "")) cursor += 1;
    let value = "";
    if (shell[cursor] === "=") {
      cursor += 1;
      while (/\s/.test(shell[cursor] ?? "")) cursor += 1;
      const quote = /['"]/.test(shell[cursor] ?? "") ? shell[cursor++] : "";
      const start2 = cursor;
      while (shell[cursor] && (quote ? shell[cursor] !== quote : !/[\s>]/.test(shell[cursor]))) cursor += 1;
      value = shell.slice(start2, cursor);
      if (quote) cursor += 1;
    }
    if (name) result.push([name, value]);
  }
  return result;
}
function isResourceAttribute(name) {
  return ["src", "srcdoc", "srcset", "href", "poster", "data"].includes(name) || name.endsWith(":href");
}
function hasUnsafeAttribute(shell) {
  return attributes(shell).some(([name, value]) => isResourceAttribute(name) || name === "style" && /\\|@import\b|url\s*\(/i.test(value));
}
function projectDataMarker(html) {
  const markers = [];
  for (let tag = nextTag(html, 0); tag; ) {
    if (!tag.closing && tag.name === "script" && attributes(tag.shell).some(([name, value]) => name === "id" && value === "project-os-data")) markers.push(tag);
    if (!tag.closing && ["script", "style"].includes(tag.name)) {
      const end = rawTextEnd(html, tag.end + 1, tag.name);
      if (tag.name === "script" && markers.at(-1) === tag) {
        const start = html.lastIndexOf("<", end);
        if (tag.shell !== markerStart || html.slice(start, end + 1) !== markerEnd) throw Error("Export marker failed round-trip verification");
        tag.content = html.slice(tag.end + 1, start);
        tag.end = end;
      }
      tag = nextTag(html, end + 1);
    } else tag = nextTag(html, tag.end + 1);
  }
  if (markers.length !== 1 || markers[0].content === void 0) throw Error("Export marker failed round-trip verification");
  return markers[0];
}
function verifyTemplate(html) {
  markerContent(html);
  for (let tag = nextTag(html, 0); tag; ) {
    if (hasUnsafeAttribute(tag.shell)) throw Error("Export retains external resources");
    if (!tag.closing && ["script", "style"].includes(tag.name)) {
      const end = rawTextEnd(html, tag.end + 1, tag.name);
      if (tag.name === "style" && /\\|@import\b|url\s*\(/i.test(html.slice(tag.end + 1, end))) throw Error("Export retains external resources");
      tag = nextTag(html, end + 1);
    } else tag = nextTag(html, tag.end + 1);
  }
}
function verifyMarkerRoundTrip(html, serialized) {
  if (JSON.stringify(JSON.parse(markerContent(html))) !== serialized) throw Error("Export marker failed round-trip verification");
}

// scripts/validate.mjs
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
var asArray2 = (value) => Array.isArray(value) ? value : [];
var hasEvidence = (node) => Array.isArray(node?.reality?.evidence) ? node.reality.evidence.length : node?.reality?.evidence;
var named = (prefix, value, fallback) => `${prefix}/${value ?? fallback}`;
var C4_VERSIONS = ["project-os-schema/v2", "project-os-schema/v2.1", "project-os-schema/v2.2"];
var KNOWN_VERSIONS = ["project-os-schema/v1", ...C4_VERSIONS];
var EPISTEMIC_VERSIONS = ["project-os-schema/v2.1", "project-os-schema/v2.2"];
function validateProvenance(node, prefix, errors, skipEvidence = false) {
  if (!skipEvidence && !String(node?.id).startsWith("template#") && !hasEvidence(node)) errors.push(`${prefix}: reality.evidence is required`);
  if (!String(node?.id).startsWith("template#") && !node?.source_trace_id) errors.push(`${prefix}: source_trace_id is required`);
}
function validateCertainty(node, prefix, errors) {
  const question = node?.certainty?.question;
  if (!question) return;
  const hypotheses = asArray2(question.hypotheses);
  if (hypotheses.length < 2) errors.push(`${prefix}: question needs at least two hypotheses`);
  validateHypotheses(hypotheses, prefix, errors);
  const priors = hypotheses.map((hypothesis) => hypothesis?.prior ?? 1 / hypotheses.length);
  if (hypotheses.length && Math.abs(priors.reduce((sum, prior) => sum + prior, 0) - 1) > 1e-12) errors.push(`${prefix}: priors must sum to one`);
  validateOutcomes(question.outcomes, hypotheses, prefix, errors);
  if (question.cost !== void 0 && (!Number.isFinite(question.cost) || question.cost < 0 || question.cost > 1)) errors.push(`${prefix}: question cost must be within [0,1]`);
}
function validateHypotheses(hypotheses, prefix, errors) {
  for (const hypothesis of hypotheses) {
    for (const field of ["id", "label", "dod", "discriminated_by"]) if (typeof hypothesis?.[field] !== "string" || !hypothesis[field]) errors.push(`${named(prefix, hypothesis?.id, "hypothesis")}: ${field} is required`);
    if (hypothesis?.prior !== void 0 && (!Number.isFinite(hypothesis.prior) || hypothesis.prior < 0 || hypothesis.prior > 1)) errors.push(`${named(prefix, hypothesis?.id, "hypothesis")}: prior must be within [0,1]`);
  }
}
function validateOutcomes(outcomes, hypotheses, prefix, errors) {
  for (const outcome of asArray2(outcomes)) if (hypotheses.some((hypothesis) => !Number.isFinite(outcome?.likelihoods?.[hypothesis?.id]) || outcome.likelihoods[hypothesis.id] < 0 || outcome.likelihoods[hypothesis.id] > 1)) errors.push(`${named(prefix, outcome?.id, "outcome")}: likelihoods must be [0,1] for every hypothesis`);
}
function validateReferences(node, prefix, boardIds, nodeIds, errors) {
  for (const ref of [...asArray2(node?.details?.fields).map((field) => field?.ref), ...asArray2(node?.details?.links).map((link) => ({ board: link?.board, node: link?.node }))]) {
    if (!ref) continue;
    if (ref.board && !boardIds.has(ref.board)) errors.push(`${prefix}: unknown board ${ref.board}`);
    if (ref.node && !nodeIds.has(ref.node)) errors.push(`${prefix}: unknown referenced node ${ref.node}`);
  }
}
var historyKinds = /* @__PURE__ */ new Set(["framed", "question", "hypothesis", "answered", "decided", "rejected", "integrated", "activated"]);
var historyTimestamp = /^\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2}))?$/;
function validateHistory(node, prefix, boardIds, nodes, errors) {
  if (node?.history === void 0) return;
  if (!Array.isArray(node.history)) return errors.push(`${prefix}: history must be an array`);
  for (const entry of node.history) {
    const entryPrefix = `${prefix}: history`;
    if (!historyTimestamp.test(entry?.ts ?? "")) errors.push(`${entryPrefix} ts is required and must be ISO8601 or YYYY-MM-DD`);
    if (!historyKinds.has(entry?.kind)) errors.push(`${entryPrefix} kind is invalid`);
    if (typeof entry?.summary !== "string" || !entry.summary) errors.push(`${entryPrefix} summary is required`);
    if (!entry?.ref && !entry?.trace && entry?.kind !== "framed") errors.push(`${entryPrefix} requires ref or trace unless framed`);
    if (!entry?.ref) continue;
    const target = nodes.get(entry.ref.node);
    if (!entry.ref.board || !entry.ref.node || !boardIds.has(entry.ref.board) || !target || target.graph !== entry.ref.board) errors.push(`${entryPrefix} ref target board and node must exist`);
  }
}
function validateNode(node, prefix, context, errors) {
  const { fullProject, graphIds, viewIds, boardIds, nodeIds, nodes, schemaVersion } = context;
  if (fullProject && (!node?.id || typeof node.id !== "string")) errors.push(`${prefix}: id is required`);
  if (EPISTEMIC_VERSIONS.includes(schemaVersion) && Object.hasOwn(node ?? {}, "superseded_by")) {
    if (typeof node.superseded_by !== "string" || !node.superseded_by) return errors.push(`${prefix}: superseded_by must be a non-empty string`);
    if (!node.graph) errors.push(`${prefix}: superseded tombstone requires graph`);
    else if (!graphIds.has(node.graph)) errors.push(`${prefix}: unknown graph ${node.graph}`);
    return;
  }
  if (fullProject) validateProvenance(node, prefix, errors, EPISTEMIC_VERSIONS.includes(schemaVersion) && node?.status === "erroneous");
  if (node?.graph && !graphIds.has(node.graph)) errors.push(`${prefix}: unknown graph ${node.graph}`);
  if (node?.graph === "blueprint" && (!node.target_axis || node.target_axis === "blueprint" || !viewIds.has(node.target_axis) || !graphIds.has(node.target_axis))) errors.push(`${prefix}: Blueprint node target axis must name an existing permanent board`);
  validateCertainty(node, prefix, errors);
  validateReferences(node, prefix, boardIds, nodeIds, errors);
  validateHistory(node, prefix, boardIds, nodes, errors);
  validateCodeFlow(node, prefix, context, errors);
}
function validateCodeFlow(node, prefix, context, errors) {
  if (node?.code_flow === void 0) return;
  const flow = node.code_flow;
  if (!flow || typeof flow !== "object" || Array.isArray(flow)) return errors.push(`${prefix}: code_flow must be an object`);
  if (flow.source !== void 0 && flow.source !== "graphify") errors.push(`${prefix}: code_flow.source must be graphify`);
  if (!Array.isArray(flow.roots) || flow.roots.length === 0) errors.push(`${prefix}: code_flow.roots must be a non-empty array`);
  else for (const root of flow.roots) if (typeof root !== "string" || !root || path.isAbsolute(root) || root.split(/[\\/]/).includes("..")) errors.push(`${prefix}: code_flow roots must be repository-relative paths`);
  if (flow.graph !== void 0 && (typeof flow.graph !== "string" || !flow.graph || path.isAbsolute(flow.graph) || flow.graph.split(/[\\/]/).includes(".."))) errors.push(`${prefix}: code_flow.graph must be a repository-relative path`);
  if (flow.max_nodes !== void 0 && (!Number.isInteger(flow.max_nodes) || flow.max_nodes < 1 || flow.max_nodes > 500)) errors.push(`${prefix}: code_flow.max_nodes must be an integer within 1..500`);
  const target = node.drilldown_view || `code-flow:${node.id}`;
  if (context.boardIds.has(target)) errors.push(`${prefix}: code_flow generated board collides with existing board ${target}`);
}
function validateEdge(edge, prefix, context, errors) {
  const { fullProject, graphIds, nodeIds, expectedGraph } = context;
  const source = edge?.source ?? edge?.from, target = edge?.target ?? edge?.to;
  if (fullProject && (!source || !target)) errors.push(`${prefix}: source and target are required`);
  if (fullProject && !edge?.source_trace_id) errors.push(`${prefix}: source_trace_id is required`);
  if (expectedGraph && edge?.graph !== expectedGraph) errors.push(`${prefix}: parked edge must be a ${expectedGraph[0].toUpperCase()}${expectedGraph.slice(1)} edge`);
  if (edge?.graph && !graphIds.has(edge.graph)) errors.push(`${prefix}: unknown graph ${edge.graph}`);
  if (source && !nodeIds.has(source)) errors.push(`${prefix}: unknown source ${source}`);
  if (target && !nodeIds.has(target)) errors.push(`${prefix}: unknown target ${target}`);
}
function validateParkedPhases(phases, context, errors) {
  for (const phase of phases) validateParkedPhase(phase, context, errors);
}
function validateParkedPhase(phase, context, errors) {
  const prefix = phase?.id ?? "phase";
  if (!["parked", "active", "integrated"].includes(phase.status)) errors.push(`${prefix}: phase status must be parked, active, or integrated`);
  if (typeof phase.memo !== "string" || !phase.memo) errors.push(`${prefix}: phase memo is required`);
  if (!phase.parked || !Array.isArray(phase.parked.nodes) || !Array.isArray(phase.parked.edges)) return errors.push(`${prefix}: parked nodes and edges are required`);
  const nodeIds = /* @__PURE__ */ new Set([...context.nodeIds, ...phase.parked.nodes.map((node) => node?.id)]);
  const payloadContext = { ...context, fullProject: true, nodeIds };
  const enforceLiveIdConflicts = phase.status === "parked";
  validateParkedNodes(phase.parked.nodes, prefix, context, payloadContext, errors, /* @__PURE__ */ new Set(), enforceLiveIdConflicts);
  validateParkedEdges(phase.parked.edges, prefix, context, payloadContext, errors, /* @__PURE__ */ new Set(), enforceLiveIdConflicts);
}
function validateParkedNodes(nodes, prefix, context, payloadContext, errors, payloadNodeIds, enforceLiveIdConflicts) {
  for (const node of nodes) {
    const nodePrefix = named(prefix, node?.id, "node");
    if (payloadNodeIds.has(node?.id)) errors.push(`${nodePrefix}: duplicate node id`);
    payloadNodeIds.add(node?.id);
    if (enforceLiveIdConflicts && (!node?.id || context.nodeIds.has(node.id))) errors.push(`${prefix}: parked node id conflicts with project`);
    if (node?.type === "phase" || node?.parked) errors.push(`${prefix}: parked phases may not nest`);
    if (node?.graph !== "blueprint") errors.push(`${nodePrefix}: parked node must be a Blueprint node`);
    validateNode(node, nodePrefix, payloadContext, errors);
  }
}
function validateParkedEdges(edges, prefix, context, payloadContext, errors, payloadEdgeIds, enforceLiveIdConflicts) {
  for (const edge of edges) {
    const edgePrefix = named(prefix, edge?.id, "edge");
    if (edge?.id && payloadEdgeIds.has(edge.id)) errors.push(`${edgePrefix}: duplicate edge id`);
    payloadEdgeIds.add(edge?.id);
    if (enforceLiveIdConflicts && edge?.id && context.edgeIds.has(edge.id)) errors.push(`${prefix}: parked edge id conflicts with project`);
    validateEdge(edge, edgePrefix, { ...payloadContext, expectedGraph: "blueprint" }, errors);
  }
}
function validateProjectOs(document) {
  const errors = [], values = projectValues(document), context = projectContext(document, values);
  validateProjectStructure(document, context.fullProject, errors);
  validateViews(values.views, errors);
  validateReservedBoards(document, context, errors);
  const phases = values.nodes.filter((node) => node?.graph === "roadmap" && node?.type === "phase");
  if (phases.filter((node) => node.status === "active").length > 1) errors.push("roadmap: at most one phase may be active");
  validateParkedPhases(phases, context, errors);
  validatePermanentNodes(values.nodes, context, errors);
  validatePermanentEdges(values.edges, context, errors);
  errors.push(...v2Diagnostics(document).errors);
  return errors;
}
function projectValues(document) {
  return { views: asArray2(document?.views), graphs: asArray2(document?.graphs), nodes: asArray2(document?.nodes), edges: asArray2(document?.edges) };
}
function projectContext(document, { views, graphs, nodes, edges }) {
  const fullProject = KNOWN_VERSIONS.includes(document?.schema_version) || document?.project !== void 0;
  const viewIds = new Set(views.map((view) => view?.id)), graphIds = new Set(graphs.map((graph) => graph?.id));
  return { fullProject, schemaVersion: document?.schema_version, graphIds, viewIds, boardIds: /* @__PURE__ */ new Set([...viewIds, ...graphIds]), nodeIds: new Set(nodes.map((node) => node?.id)), nodes: new Map(nodes.map((node) => [node?.id, node])), edgeIds: new Set(edges.map((edge) => edge?.id)) };
}
function validateProjectStructure(document, fullProject, errors) {
  if (!fullProject) return;
  for (const field of ["schema_version", "project", "views", "graphs", "nodes", "edges"]) if (document?.[field] === void 0) errors.push(`${field} is required`);
  if (!KNOWN_VERSIONS.includes(document.schema_version)) errors.push(`schema_version must be one of ${KNOWN_VERSIONS.join(", ")}`);
  for (const field of ["views", "graphs", "nodes", "edges"]) if (!Array.isArray(document?.[field])) errors.push(`${field} must be an array`);
}
function validateViews(views, errors) {
  for (const view of views) if (view?.layout !== void 0 && view.layout !== "manual") errors.push(`${view?.id ?? "view"}: layout must be manual`);
}
function validateReservedBoards(document, { fullProject, viewIds, graphIds }, errors) {
  if (!(fullProject || document?.views || document?.graphs)) return;
  if (!viewIds.has("blueprint")) errors.push("blueprint: reserved board is required in views");
  if (!graphIds.has("blueprint")) errors.push("blueprint: reserved board is required in graphs");
  if (fullProject && !viewIds.has("roadmap")) errors.push("roadmap: reserved board is required in views");
  if (fullProject && !graphIds.has("roadmap")) errors.push("roadmap: reserved board is required in graphs");
}
function validatePermanentNodes(nodes, context, errors) {
  const ids = /* @__PURE__ */ new Set();
  for (const node of nodes) {
    validateNode(node, node?.id ?? "node", context, errors);
    if (ids.has(node?.id)) errors.push(`${node?.id}: duplicate node id`);
    ids.add(node?.id);
  }
}
function validatePermanentEdges(edges, context, errors) {
  const ids = /* @__PURE__ */ new Set();
  for (const edge of edges) {
    validateEdge(edge, edge?.id ?? "edge", context, errors);
    if (edge?.id && ids.has(edge.id)) errors.push(`${edge.id}: duplicate edge id`);
    ids.add(edge?.id);
  }
}
if (import.meta.url.endsWith("/validate.mjs") && process.argv[1] === new URL(import.meta.url).pathname) {
  const input = process.argv[2];
  if (input) try {
    const yaml2 = createRequire(new URL("../app/package.json", import.meta.url))("js-yaml");
    const document = yaml2.load(fs.readFileSync(path.resolve(input), "utf8"));
    const diagnostics = v2Diagnostics(document), errors = validateProjectOs(document), warnings = diagnostics.warnings;
    if (errors.length) {
      console.error(errors.join("\n"));
      process.exitCode = 2;
    } else if (process.argv[3] === "--gate") {
      const summary = gateSummary(document);
      console.log(JSON.stringify(summary));
      if (summary.gate === "armed-blocked") process.exitCode = 4;
    } else {
      if (warnings.length) console.error(warnings.join("\n"));
      if (diagnostics.clustering?.length) console.log(`clustering-honesty: ${diagnostics.clustering.join("; ")}`);
    }
  } catch (error) {
    process.stderr.write(`Unable to read input: ${error.message}
`);
    process.exitCode = 3;
  }
  else {
    process.stderr.write("Usage: validate.mjs <project.yaml>\n");
    process.exitCode = 3;
  }
}
function gateSummary(document) {
  const nodes = asArray2(document?.nodes);
  const count = (status) => nodes.filter((node) => node?.status === status).length;
  const erroneous = count("erroneous"), blocking = nodes.filter((node) => node?.status === "open-question" && node?.blocking === true).length;
  const triaged = typeof document?.gate?.triaged === "string";
  return { schema: document?.schema_version, erroneous, blocking, open_questions: count("open-question"), hypotheses: count("hypothesis"), planned: count("planned"), triaged, gate: !triaged ? "advisory" : erroneous || blocking ? "armed-blocked" : "armed-clear" };
}

// scripts/code-flow.mjs
import { lstatSync, readFileSync, realpathSync } from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";
var defaultRelations = /* @__PURE__ */ new Set(["calls", "contains", "method", "imports_from"]);
var defaultMaxNodes = 160;
function hasCodeFlow(document) {
  return Array.isArray(document?.nodes) && document.nodes.some((node) => node?.code_flow);
}
function projectCodeFlows(document, projectRoot, { readGraph = readGraphFile } = {}) {
  if (!hasCodeFlow(document)) return document;
  const projected = structuredClone(document);
  const declarations = projected.nodes.filter((node) => node?.code_flow);
  const graphCache = /* @__PURE__ */ new Map();
  for (const service of declarations) {
    const specification = validateSpecification(service);
    const graphPath = resolve(projectRoot, specification.graph);
    const graph = graphCache.get(graphPath) ?? readGraph(graphPath, projectRoot);
    graphCache.set(graphPath, graph);
    appendProjection(projected, service, specification, graph, projectRoot);
  }
  return projected;
}
function validateSpecification(service) {
  const value = service.code_flow;
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw Error(`${service.id}: code_flow must be an object`);
  }
  if (value.source !== void 0 && value.source !== "graphify") {
    throw Error(`${service.id}: code_flow.source must be graphify`);
  }
  if (!Array.isArray(value.roots) || value.roots.length === 0) {
    throw Error(`${service.id}: code_flow.roots must contain at least one repository-relative path`);
  }
  const roots = value.roots.map((root) => normalizeRoot(root, service.id));
  const maxNodes = value.max_nodes ?? defaultMaxNodes;
  if (!Number.isInteger(maxNodes) || maxNodes < 1 || maxNodes > 500) {
    throw Error(`${service.id}: code_flow.max_nodes must be an integer within 1..500`);
  }
  const graph = value.graph ?? "graphify-out/graph.json";
  if (typeof graph !== "string" || !graph || isAbsolute(graph) || escapes(graph)) {
    throw Error(`${service.id}: code_flow.graph must be a repository-relative path`);
  }
  return { roots, maxNodes, graph };
}
function normalizeRoot(value, serviceId) {
  if (typeof value !== "string" || !value.trim() || isAbsolute(value) || escapes(value)) {
    throw Error(`${serviceId}: code_flow roots must be repository-relative paths`);
  }
  return value.replaceAll("\\", "/").replace(/^\.\/|\/$/g, "");
}
function escapes(value) {
  return value.split(/[\\/]/).some((part) => part === "..");
}
function readGraphFile(file, projectRoot) {
  try {
    if (lstatSync(file).isSymbolicLink()) throw Error("graph file must not be a symlink");
    const realProject = realpathSync(projectRoot);
    const realGraph = realpathSync(file);
    const distance = relative(realProject, realGraph);
    if (isAbsolute(distance) || distance === ".." || distance.startsWith(`..${sep}`)) {
      throw Error("graph path resolves outside the project");
    }
    const graph = JSON.parse(readFileSync(file, "utf8"));
    if (!Array.isArray(graph?.nodes) || !Array.isArray(graph?.links)) {
      throw Error("expected nodes[] and links[]");
    }
    return graph;
  } catch (error) {
    throw Error(`Unable to read Graphify projection ${file}: ${error.message}`);
  }
}
function appendProjection(document, service, specification, graph, projectRoot) {
  const boardId = service.drilldown_view || `code-flow:${service.id}`;
  service.drilldown_view = boardId;
  ensureBoard(document, boardId, service);
  const graphNodes = new Map(graph.nodes.map((node) => [node.id, node]));
  const selected = graph.nodes.filter((node) => sourceInRoots(node.source_file, specification.roots, projectRoot)).sort(compareGraphNodes);
  if (selected.length === 0) {
    throw Error(`${service.id}: no Graphify nodes matched code_flow.roots (${specification.roots.join(", ")})`);
  }
  if (selected.length > specification.maxNodes) {
    throw Error(`${service.id}: code flow has ${selected.length} nodes, above max_nodes=${specification.maxNodes}; narrow code_flow.roots instead of publishing a truncated graph`);
  }
  const selectedIds = new Set(selected.map((node) => node.id));
  const links = graph.links.filter((link) => defaultRelations.has(link.relation)).map((link) => semanticLink(link, projectRoot)).filter((link) => selectedIds.has(link.source) && selectedIds.has(link.target)).sort(compareLinks);
  for (const node of selected) document.nodes.push(projectedNode(service, boardId, node, graph.links, projectRoot));
  for (const [index, link] of links.entries()) document.edges.push(projectedEdge(service, boardId, link, index));
}
function sourceInRoots(sourceFile, roots, projectRoot) {
  if (typeof sourceFile !== "string" || !sourceFile) return false;
  const normalized = isAbsolute(sourceFile) ? repositoryRelative(sourceFile, projectRoot) : sourceFile.replaceAll("\\", "/").replace(/^\.\/+/, "");
  if (!normalized) return false;
  return roots.some((root) => normalized === root || normalized.startsWith(`${root}/`));
}
function repositoryRelative(sourceFile, projectRoot) {
  const candidate = relative(resolve(projectRoot), resolve(sourceFile));
  if (!candidate || isAbsolute(candidate) || candidate === ".." || candidate.startsWith(`..${sep}`)) return null;
  return candidate.split(sep).join("/");
}
function ensureBoard(document, boardId, service) {
  if (document.views.some((view) => view.id === boardId) || document.graphs.some((graph) => graph.id === boardId)) {
    throw Error(`${service.id}: generated code-flow board collides with existing board ${boardId}`);
  }
  document.views.push({
    id: boardId,
    label: `${service.label} \xB7 code flow`,
    type: "code-flow",
    memo: "Static Graphify projection. Follow calls from entry points toward storage boundaries; dynamic dispatch may be absent."
  });
  document.graphs.push({ id: boardId, view: boardId, derived_from: "graphify" });
}
function projectedNode(service, boardId, node, allLinks, projectRoot) {
  const incomingContainment = allLinks.some((link) => ["contains", "method"].includes(link.relation) && link._tgt === node.id);
  const outgoingContainment = allLinks.some((link) => ["contains", "method"].includes(link.relation) && link._src === node.id);
  const type2 = String(node.label ?? "").endsWith("()") || incomingContainment ? "method" : outgoingContainment ? "component" : "code";
  const trace = `graphify-out/graph.json#${node.id}`;
  const location = [displayPath(node.source_file, projectRoot), node.source_location].filter(Boolean).join(":");
  return {
    id: projectionId(service.id, node.id),
    graph: boardId,
    label: String(node.label ?? node.id),
    type: type2,
    summary: location || "Static code symbol",
    source_trace_id: trace,
    reality: { kind: "inferred", confidence: "medium", evidence: [trace] },
    details: {
      sections: [
        { title: "Code location", content: location || "Graphify did not provide a source location." },
        { title: "Analysis boundary", content: "This is a static call projection. Reflection, runtime routing, events, SQL strings, and framework injection may create paths not visible here." }
      ]
    }
  };
}
function semanticLink(link, projectRoot) {
  const location = [displayPath(link.source_file, projectRoot), link.source_location].filter(Boolean).join(":");
  return {
    relation: link.relation,
    source: link._src ?? link.source,
    target: link._tgt ?? link.target,
    trace: `graphify-out/graph.json#${location || "link"}`
  };
}
function projectedEdge(service, boardId, link, index) {
  return {
    id: `codeflow#${service.id}#edge#${index + 1}`,
    graph: boardId,
    source: projectionId(service.id, link.source),
    target: projectionId(service.id, link.target),
    type: link.relation === "imports_from" ? "uses" : link.relation,
    label: link.relation.replace("_", " "),
    source_trace_id: link.trace
  };
}
function projectionId(serviceId, graphifyId) {
  return `codeflow#${serviceId}#${graphifyId}`;
}
function displayPath(value, projectRoot) {
  if (typeof value !== "string") return "";
  if (isAbsolute(value)) return repositoryRelative(value, projectRoot) ?? "[external source]";
  return value.replaceAll("\\", "/");
}
function compareGraphNodes(left, right) {
  return String(left.source_file ?? "").localeCompare(String(right.source_file ?? "")) || String(left.source_location ?? "").localeCompare(String(right.source_location ?? "")) || String(left.id).localeCompare(String(right.id));
}
function compareLinks(left, right) {
  return left.source.localeCompare(right.source) || left.target.localeCompare(right.target) || left.relation.localeCompare(right.relation);
}

// scripts/export-lite.mjs
var expectedTemplateHash = "c8c0e6de1e412082f3ec5404d310e2823005206694845e1e5350566dae8976fe";
function exportLite(input, templateFile) {
  const project = realDirectory(input, "project root");
  const opencode = realDirectory(join(project, ".opencode"), ".opencode");
  const yamlFile = regularFile(join(opencode, "project-os.yaml"), "YAML input");
  const templateBytes = readFileSync2(regularFile(templateFile, "template"));
  if (createHash("sha256").update(templateBytes).digest("hex") !== expectedTemplateHash) fail(3, "Export template integrity check failed");
  const template = templateBytes.toString("utf8");
  const document = parseYaml(readFileSync2(yamlFile, "utf8"));
  const errors = exportErrors(document).concat(validateProjectOs(document));
  if (errors.length) fail(2, errors.join("\n"));
  const serialized = JSON.stringify(projectCodeFlows(document, project));
  const html = injectProjectData(template, serialized);
  verifyMarkerRoundTrip(html, serialized);
  writeAtomic(join(opencode, "project-os.html"), html);
}
function parseYaml(source) {
  try {
    return load(source);
  } catch (error) {
    fail(3, `Unable to parse YAML: ${error.message}`);
  }
}
function writeAtomic(output, data) {
  if (lstatSync2(dirname(output)).isSymbolicLink()) fail(3, "Output directory must not be a symlink");
  if (!existsRegular(output)) fail(3, "Output must be a regular non-symlink file");
  const temporary = join(dirname(output), `.${basename(output)}.${process.pid}.tmp`);
  let fd;
  try {
    fd = openSync(temporary, "wx", 384);
    writeSync(fd, data);
    fsyncSync(fd);
    closeSync(fd);
    renameSync(temporary, output);
  } catch (error) {
    if (fd !== void 0) closeSync(fd);
    rmSync(temporary, { force: true });
    fail(3, `Unable to write export: ${error.message}`);
  }
}
function existsRegular(value) {
  try {
    const stat = lstatSync2(value);
    return !stat.isSymbolicLink() && stat.isFile();
  } catch {
    return true;
  }
}
function realDirectory(value, name) {
  try {
    noAncestorSymlinks(value);
    const stat = lstatSync2(value);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw Error();
    return resolve2(value);
  } catch (error) {
    fail(3, error.message?.includes("symlink") ? error.message : `${name} must be a real directory`);
  }
}
function noAncestorSymlinks(value) {
  const absolute = resolve2(value), { base, root } = ancestorPath(absolute);
  let current = base ? realpathSync2(base) : root;
  for (const part of pathParts(absolute, root)) {
    current = join(current, part);
    if (lstatSync2(current).isSymbolicLink()) throw Error(`project path contains symlink: ${current}`);
  }
}
function ancestorPath(absolute, candidates = [tmpdir(), "/tmp", process.env.HOME], path2 = { isAbsolute: isAbsolute2, parse, relative: relative2, sep: sep2 }) {
  const base = candidates.find((root) => {
    if (!root) return false;
    const distance = path2.relative(root, absolute);
    return !path2.isAbsolute(distance) && distance !== ".." && !distance.startsWith(`..${path2.sep}`);
  });
  return { base, root: base ?? path2.parse(absolute).root };
}
function pathParts(value, root, path2 = { relative: relative2, sep: sep2 }) {
  return path2.relative(root, value).split(path2.sep).filter(Boolean);
}
function regularFile(value, name) {
  try {
    const stat = lstatSync2(value);
    if (stat.isSymbolicLink() || !stat.isFile()) throw Error();
    return value;
  } catch {
    fail(3, `${name} must be a regular non-symlink file`);
  }
}
function fail(code, message) {
  const error = Error(message);
  error.code = code;
  throw error;
}
if (process.argv[1] && realpathSync2(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    exportLite(process.argv[2], process.argv[3]);
  } catch (error) {
    process.stderr.write(`${error.message}
`);
    process.exitCode = error.code ?? 3;
  }
}
export {
  ancestorPath,
  exportLite,
  pathParts
};
