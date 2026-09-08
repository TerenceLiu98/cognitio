import * as ExternalPlugin from "./.quartz/plugins"
import { loadQuartzConfig, loadQuartzLayout } from "./quartz/plugins/loader/config-loader"
import { registerCondition } from "./quartz/plugins/loader/conditions"

type ExplorerNode = {
  slugSegment?: string
}

type WikiPage = {
  slug?: string
}

registerCondition("index-only", ({ fileData }) => fileData.slug === "index")

ExternalPlugin.Explorer({
  filterFn: (node: ExplorerNode) => node.slugSegment !== "tags" && node.slugSegment !== "papers",
})

ExternalPlugin.RecentNotes({
  title: "Papers",
  limit: 1000,
  linkToMore: false,
  showTags: false,
  hideTagPages: true,
  hideFolderPages: true,
  filter: (page: WikiPage) => page.slug?.startsWith("papers/") ?? false,
})

const config = await loadQuartzConfig()
export default config
export const layout = await loadQuartzLayout()
