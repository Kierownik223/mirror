if (!window.JSON) {
  window.JSON = {
    parse: function (s) {
      return eval('(' + s + ')');
    },
    stringify: function (obj) {
      var t = typeof obj;
      if (t !== "object" || obj === null) {
        if (t === "string") return '"' + obj.replace(/"/g, '\\"') + '"';
        return String(obj);
      }
      var json = [];
      var isArray = Object.prototype.toString.call(obj) === "[object Array]";
      for (var key in obj) {
        if (Object.prototype.hasOwnProperty.call(obj, key)) {
          var value = obj[key];
          var vtype = typeof value;
          if (vtype === "string") value = '"' + value.replace(/"/g, '\\"') + '"';
          else if (vtype === "object" && value !== null) value = arguments.callee(value);
          json.push((isArray ? "" : '"' + key + '":') + String(value));
        }
      }
      return (isArray ? "[" : "{") + json.join(",") + (isArray ? "]" : "}");
    }
  };
}