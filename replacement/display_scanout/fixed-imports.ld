/* The scanout exports in the owned fixed range and the stock services they call. Retained callers
   reach the exports through one A32 branch published at each stock entry (`artifact publications`). */
EXTERN(xm1_scanout_isr_one xm1_scanout_isr_zero xm1_scanout_scheduler_register xm1_scanout_capture
  xm1_scanout_item_configuration xm1_scanout_range_packet xm1_scanout_range_enabled xm1_scanout_group_options
  xm1_scanout_commit xm1_scanout_commit_zero xm1_scanout_commit_one xm1_scanout_blank)
scanout_enter_critical = 0x007235d8;
scanout_leave_critical = 0x007236b0;
scanout_yield = 0x00723ce8;
scanout_irq_configure = 0x0064e918;
scanout_irq_release = 0x0064eb20;
