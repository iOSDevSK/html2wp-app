export type Page={key:string;title:string;kind:string;reviewedRevision:number|null;note:string;image:string|null};
export type Gate={name:string;status:'passed'|'failed'|'not_run';detail:string};
/** A file a run delivered. `checks`: the run that made it (flash, full, h2g; older releases wrote others). */
export type Artifact={id:string;revision:number;filename:string;sha256:string;createdAt:string;kind:'theme'|'pdf'|'report'|'editor'|'astro'|'summary'|'file';reviewed:boolean;checks?:string};
export type ThemeTarget='html'|'gutenberg'|'astro'|'h2g';
export type Project={target?:ThemeTarget;flash?:boolean;model?:string|null;effort?:string|null;autoApprove?:boolean;conversionApprovalRequired?:boolean;archived?:boolean;id:string;name:string;sourceName:string;kind:string;createdAt:string;updatedAt:string;phase:string;lastStep?:string|null;revision:number;threadId:string|null;pages:Page[];gates:Gate[];artifacts:Artifact[];preview:{url:string;username:string;running:boolean}|null;runtimeImage:string;pluginCommit:string;reporting:string;lastError:string|null};
/** The preview WordPress the plugin started (preview::status). */
export type PreviewSite={available:boolean;url?:string;user?:string;password?:string;running?:boolean;project?:string};
/** Preview opening status. The system browser does not load a managed extension. */
export type PreviewBrowser={browser?:string|null;running?:boolean;extension:{version:string;sha:string|null}|null;note?:string|null};
export type Message={id:string;projectId:string;role:string;text:string;createdAt:string;action?:'exports'};
export type Activity={id:string;projectId:string;label:string;status:string;createdAt:string};
export type Runtime={ready:boolean;docker:boolean;imageReady:boolean;pluginReady?:boolean;message:string;version?:string;architecture?:string};
export type ChromeBridge={paired:boolean;code:string|null;port:number|null};
export type Bootstrap={experimentalGutenberg?:boolean;activeProject?:string|null;selectedModel?:string;projects:Project[];versions:{appVersion?:string;pluginVersion:string;pluginCommit:string;codexVersion:string;adapterVersion:string};platform:string;architecture:string;disclosureAccepted:boolean;licenceConfigured:boolean;activeProjects?:string[];maxParallel?:number;maxParallelLimit?:number};
export type Account={account:null|{type:string;email?:string;planType?:string};requiresOpenaiAuth?:boolean;limits?:{ordinaryUsageAllowed:boolean|null;rateLimits?:{primary?:{usedPercent:number;resetsAt:number|null}|null;secondary?:{usedPercent:number;resetsAt:number|null}|null}}};
export type Login={loginId:string;verificationUrl:string;userCode:string};
export type Question={id:string;params:{threadId?:string;questions:{id:string;header:string;question:string;options?:{label:string;description:string}[]}[]}};

export type LicenceStatus={mode:"free"|"licensed";state:string;checkedAt:string;expiresAt?:string|null;neverExpires?:boolean;plan?:string|null;creditLine?:string|null;note?:string|null;message:string;lastKnown?:LicenceStatus|null};

export type CodexModel=Pick<import("../schemas/codex/v2/Model").Model,"id"|"model"|"displayName"|"description"|"isDefault"|"hidden"|"supportedReasoningEfforts"|"defaultReasoningEffort">;
export type ModelCatalog={models:CodexModel[];selectedModel:string;selectedEffort:string};
