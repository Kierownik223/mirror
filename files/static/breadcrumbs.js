function createBreadcrumbs(path) {
    var container = document.createElement('span');
    container.className = 'breadcrumbs';
    container.id = 'breadcrumbs';

    container.appendChild(document.createTextNode('/'));

    if (path && path !== "/") {
        var segments = path.split('/');
        var numSegments = segments.length;

        for (var i = 0; i < numSegments; i++) {
            var segment = segments[i];
            if (segment === "") continue;

            var subpathArr = segments.slice(0, i + 1);
            var subpath = subpathArr.join('/');

            if (i === numSegments - 1) {
                var span = document.createElement('span');
                span.innerText = segment;
                container.appendChild(span);
            } else {
                var span2 = document.createElement('span');
                var a = document.createElement('a');

                a.href = encodeURIComponent(subpath) + '/';
                a.innerText = segment;
                span2.appendChild(a);

                span2.appendChild(document.createTextNode('/'));

                container.appendChild(span2);
            }
        }
    }

    return container;
}
