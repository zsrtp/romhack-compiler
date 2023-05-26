var releases = new Map();

window.onload = function () {
  
  const url = "https://api.github.com/repos/zsrtww/tww-gz/releases";

  fetch(url)
    .then((resp) => resp.json())
    .then(function (data) {
      for (const release of data) {
        let downloadUrls = new Map();
        for (const url of release.assets) {
          downloadUrls.set(url.name, url.browser_download_url);
        }
        releases.set(release.tag_name, downloadUrls);
      }

      let keys = [...releases.keys()];

      let options = keys
        .map((item) => `<option value=${item.toLowerCase()}>${item}</option>`)
        .join("\n");
      document.querySelector("select").innerHTML = options;
    })
    .catch(function (error) {
      console.log(error);
    });
};
